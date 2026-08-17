/*
 * Bounded native I/O worker pool.
 *
 * A pool belongs to one runtime boot.  Workers receive only heap-owned plain
 * data and never call into the Lira allocator, scheduler, generated code, or
 * context-switch machinery.  Fatal runtime teardown can orphan a pool: the
 * scheduler detaches it from the next boot and workers discard their results
 * when they eventually return, so an uncancellable syscall cannot retain a
 * freed fiber, JIT module, or GC heap.
 */
#include "lira_rt.h"

#include <errno.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define LIRA_IO_WORKERS 4
#define LIRA_IO_QUEUE_LIMIT 128

typedef struct LiraIoPool LiraIoPool;
typedef struct LiraIoCompletion LiraIoCompletion;
typedef struct LiraIoJob LiraIoJob;

struct LiraIoJob {
    LiraIoWorkFn work;
    LiraIoDestroyFn destroy_arg;
    void *arg;
    LiraIoCompleteFn complete;
    LiraIoDestroyFn destroy_result;
    void *owner;
    uint64_t generation;
    LiraIoJob *next;
    LiraIoCompletion *completion;
};

struct LiraIoCompletion {
    LiraIoCompleteFn complete;
    LiraIoDestroyFn destroy_result;
    void *owner;
    uint64_t generation;
    void *result;
    void *failure_arg;
    int status;
    /* On worker-side result allocation failure, retain the job argument until
     * the scheduler callback has reset any busy handle state. */
    int result_is_arg;
    LiraIoCompletion *next;
    LiraIoJob *job;
};

struct LiraIoPool {
    pthread_mutex_t lock;
    pthread_cond_t cond;
    pthread_t threads[LIRA_IO_WORKERS];
    int thread_count;
    int active;
    int stopping;
    int orphaned;
    _Atomic int cancel;
    size_t queued;
    size_t pending;
    LiraIoJob *head;
    LiraIoJob *tail;
    LiraIoCompletion *done_head;
    LiraIoCompletion *done_tail;
    LiraIoPool *next_orphan;
};

/* Only the scheduler thread reads or replaces this pointer.  Workers retain
 * their own pool argument and use the thread-local cancellation view below;
 * they never observe this scheduler global. */
static LiraIoPool *g_io_pool = NULL;
static LiraIoPool *g_io_orphans = NULL;
static _Thread_local LiraIoPool *g_io_worker_pool = NULL;

static void destroy_job(LiraIoJob *job) {
    if (job == NULL) {
        return;
    }
    if (job->destroy_arg != NULL) {
        job->destroy_arg(job->arg);
    }
    lira_rt_mem_free(job->completion);
    lira_rt_mem_free(job);
}

static void destroy_completion(LiraIoCompletion *done) {
    if (done == NULL) {
        return;
    }
    if (done->result_is_arg) {
        if (done->job != NULL && done->job->destroy_arg != NULL) {
            done->job->destroy_arg(done->failure_arg);
        }
    } else if (done->destroy_result != NULL) {
        done->destroy_result(done->result);
    }
    lira_rt_mem_free(done->job);
    lira_rt_mem_free(done);
}

static void free_orphan_pool(LiraIoPool *pool) {
    pthread_mutex_destroy(&pool->lock);
    pthread_cond_destroy(&pool->cond);
    lira_rt_mem_free(pool);
}

static void *lira_io_worker(void *arg) {
    LiraIoPool *pool = (LiraIoPool *)arg;
    g_io_worker_pool = pool;
    for (;;) {
        pthread_mutex_lock(&pool->lock);
        while (pool->head == NULL && !pool->stopping) {
            pthread_cond_wait(&pool->cond, &pool->lock);
        }
        if (pool->head == NULL && pool->stopping) {
            pool->active--;
            pthread_mutex_unlock(&pool->lock);
            g_io_worker_pool = NULL;
            return NULL;
        }
        LiraIoJob *job = pool->head;
        pool->head = job->next;
        if (pool->head == NULL) {
            pool->tail = NULL;
        }
        job->next = NULL;
        pool->queued--;
        pthread_mutex_unlock(&pool->lock);

        void *result = NULL;
        int status = job->work(job->arg, &result);
        /* Successful jobs must publish a result object. Preserve the argument
         * on a broken success contract so the scheduler-side callback can
         * restore any checked-out handle state before ownership is released. */
        if (status == 0 && result == NULL) {
            status = -1;
        }
        int result_is_arg = result == NULL && status != 0;
        if (!result_is_arg && job->destroy_arg != NULL) {
            job->destroy_arg(job->arg);
        }

        LiraIoCompletion *done = job->completion;
        done->complete = job->complete;
        done->destroy_result = job->destroy_result;
        done->owner = job->owner;
        done->generation = job->generation;
        done->result = result;
        done->failure_arg = result_is_arg ? job->arg : NULL;
        done->status = status;
        done->result_is_arg = result_is_arg;
        done->next = NULL;
        done->job = job;

        pthread_mutex_lock(&pool->lock);
        if (pool->orphaned) {
            pthread_mutex_unlock(&pool->lock);
            destroy_completion(done);
            continue;
        }
        if (pool->done_tail == NULL) {
            pool->done_head = pool->done_tail = done;
        } else {
            pool->done_tail->next = done;
            pool->done_tail = done;
        }
        pthread_cond_broadcast(&pool->cond);
        pthread_mutex_unlock(&pool->lock);
    }
}

int lira_io_start(void) {
    if (g_io_pool != NULL) {
        return 0;
    }
    LiraIoPool *pool = (LiraIoPool *)lira_rt_mem_try_alloc(sizeof(LiraIoPool), 1);
    if (pool == NULL) {
        return -1;
    }
    if (pthread_mutex_init(&pool->lock, NULL) != 0) {
        lira_gc_note_allocation_failure("could not initialize I/O worker mutex");
        lira_rt_mem_free(pool);
        return -1;
    }
    if (pthread_cond_init(&pool->cond, NULL) != 0) {
        lira_gc_note_allocation_failure("could not initialize I/O worker condition variable");
        pthread_mutex_destroy(&pool->lock);
        lira_rt_mem_free(pool);
        return -1;
    }
    atomic_store_explicit(&pool->cancel, 0, memory_order_release);
    pool->active = LIRA_IO_WORKERS;
    for (int i = 0; i < LIRA_IO_WORKERS; i++) {
        if (pthread_create(&pool->threads[i], NULL, lira_io_worker, pool) != 0) {
            lira_gc_note_allocation_failure("could not start I/O worker thread");
            pthread_mutex_lock(&pool->lock);
            pool->stopping = 1;
            pool->active = i;
            pthread_cond_broadcast(&pool->cond);
            pthread_mutex_unlock(&pool->lock);
            for (int j = 0; j < i; j++) {
                pthread_join(pool->threads[j], NULL);
            }
            free_orphan_pool(pool);
            return -1;
        }
        pool->thread_count++;
    }
    g_io_pool = pool;
    return 0;
}

int lira_io_submit(LiraIoWorkFn work, void *arg, LiraIoDestroyFn destroy_arg,
                   LiraIoCompleteFn complete, LiraIoDestroyFn destroy_result,
                   void *owner, uint64_t generation) {
    if (work == NULL || complete == NULL || g_io_pool == NULL) {
        return -1;
    }
    LiraIoJob *job = (LiraIoJob *)lira_rt_mem_try_alloc(sizeof(LiraIoJob), 1);
    if (job == NULL) {
        return -1;
    }
    job->completion =
        (LiraIoCompletion *)lira_rt_mem_try_alloc(sizeof(LiraIoCompletion), 1);
    if (job->completion == NULL) {
        lira_rt_mem_free(job);
        return -1;
    }
    job->work = work;
    job->destroy_arg = destroy_arg;
    job->arg = arg;
    job->complete = complete;
    job->destroy_result = destroy_result;
    job->owner = owner;
    job->generation = generation;

    LiraIoPool *pool = g_io_pool;
    pthread_mutex_lock(&pool->lock);
    if (pool->stopping || pool->orphaned || pool->queued >= LIRA_IO_QUEUE_LIMIT) {
        pthread_mutex_unlock(&pool->lock);
        /* Submission failure does not consume arg ownership.  Every wrapper
         * destroys its argument on the non-parked path; consuming it here
         * would double-free when the queue is full or stopping. */
        lira_rt_mem_free(job->completion);
        lira_rt_mem_free(job);
        return -1;
    }
    if (pool->tail == NULL) {
        pool->head = pool->tail = job;
    } else {
        pool->tail->next = job;
        pool->tail = job;
    }
    pool->queued++;
    pool->pending++;
    pthread_cond_signal(&pool->cond);
    pthread_mutex_unlock(&pool->lock);
    return 0;
}

size_t lira_io_pending(void) {
    LiraIoPool *pool = g_io_pool;
    if (pool == NULL) {
        return 0;
    }
    pthread_mutex_lock(&pool->lock);
    size_t pending = pool->pending;
    pthread_mutex_unlock(&pool->lock);
    return pending;
}

void lira_io_wait(void) {
    LiraIoPool *pool = g_io_pool;
    if (pool == NULL) {
        return;
    }
    pthread_mutex_lock(&pool->lock);
    while (pool->done_head == NULL && pool->pending != 0) {
        pthread_cond_wait(&pool->cond, &pool->lock);
    }
    pthread_mutex_unlock(&pool->lock);
}

size_t lira_io_drain(void) {
    LiraIoPool *pool = g_io_pool;
    if (pool == NULL) {
        return 0;
    }
    pthread_mutex_lock(&pool->lock);
    LiraIoCompletion *done = pool->done_head;
    pool->done_head = pool->done_tail = NULL;
    size_t detached = 0;
    for (LiraIoCompletion *it = done; it != NULL; it = it->next) {
        detached++;
    }
    if (detached <= pool->pending) {
        pool->pending -= detached;
    } else {
        pool->pending = 0;
    }
    pthread_mutex_unlock(&pool->lock);

    size_t count = 0;
    while (done != NULL) {
        LiraIoCompletion *next = done->next;
        done->complete(done->owner, done->generation, done->result, done->status,
                       done->failure_arg);
        if (done->result_is_arg) {
            if (done->job != NULL && done->job->destroy_arg != NULL) {
                done->job->destroy_arg(done->failure_arg);
            }
        } else if (done->destroy_result != NULL) {
            done->destroy_result(done->result);
        }
        lira_rt_mem_free(done->job);
        lira_rt_mem_free(done);
        done = next;
        count++;
    }
    return count;
}

void lira_io_shutdown(void) {
    LiraIoPool *pool = g_io_pool;
    if (pool == NULL) {
        return;
    }
    pthread_mutex_lock(&pool->lock);
    pool->stopping = 1;
    atomic_store_explicit(&pool->cancel, 1, memory_order_release);
    pthread_cond_broadcast(&pool->cond);
    int thread_count = pool->thread_count;
    pthread_mutex_unlock(&pool->lock);
    for (int i = 0; i < thread_count; i++) {
        pthread_join(pool->threads[i], NULL);
    }

    pthread_mutex_lock(&pool->lock);
    while (pool->head != NULL) {
        LiraIoJob *job = pool->head;
        pool->head = job->next;
        destroy_job(job);
    }
    pool->tail = NULL;
    pool->queued = 0;
    while (pool->done_head != NULL) {
        LiraIoCompletion *done = pool->done_head;
        pool->done_head = done->next;
        destroy_completion(done);
    }
    pool->done_tail = NULL;
    pool->pending = 0;
    pthread_mutex_unlock(&pool->lock);
    g_io_pool = NULL;
    free_orphan_pool(pool);
}

/* Abort is used only after a runtime failure.  It never waits for a worker:
 * workers discard their result and free this pool after the last active job
 * returns.  The global is cleared before the caller frees fibers/JIT state. */
void lira_io_abort(void) {
    LiraIoPool *pool = g_io_pool;
    if (pool == NULL) {
        return;
    }
    g_io_pool = NULL;
    pthread_mutex_lock(&pool->lock);
    pool->orphaned = 1;
    pool->stopping = 1;
    atomic_store_explicit(&pool->cancel, 1, memory_order_release);
    while (pool->head != NULL) {
        LiraIoJob *job = pool->head;
        pool->head = job->next;
        destroy_job(job);
    }
    pool->tail = NULL;
    pool->queued = 0;
    while (pool->done_head != NULL) {
        LiraIoCompletion *done = pool->done_head;
        pool->done_head = done->next;
        destroy_completion(done);
    }
    pool->done_tail = NULL;
    pool->pending = 0;
    pthread_cond_broadcast(&pool->cond);
    /* Detach while holding the pool lock.  Workers cannot reach the last-
     * worker free path until this loop has finished, so the thread handles
     * remain valid even when all jobs were already idle at abort time. */
    for (int i = 0; i < pool->thread_count; i++) {
        pthread_detach(pool->threads[i]);
    }
    pthread_mutex_unlock(&pool->lock);
    pool->next_orphan = g_io_orphans;
    g_io_orphans = pool;
}

int lira_io_reap_orphans(void) {
    int all_reaped = 1;
    LiraIoPool **link = &g_io_orphans;
    while (*link != NULL) {
        LiraIoPool *pool = *link;
        pthread_mutex_lock(&pool->lock);
        int done = pool->active == 0;
        pthread_mutex_unlock(&pool->lock);
        if (!done) {
            all_reaped = 0;
            link = &pool->next_orphan;
            continue;
        }
        *link = pool->next_orphan;
        free_orphan_pool(pool);
    }
    return all_reaped;
}

int lira_io_cancelled(void) {
    LiraIoPool *pool = g_io_worker_pool;
    return pool != NULL &&
           atomic_load_explicit(&pool->cancel, memory_order_acquire) != 0;
}

int lira_io_orphaned(void) {
    LiraIoPool *pool = g_io_worker_pool;
    return pool != NULL && pool->orphaned;
}

/* Opt-in fault injection for native runtime tests. */
int lira_io_test_fail_result_alloc(const char *name) {
    const char *value = name == NULL ? NULL : getenv(name);
    return value != NULL && strcmp(value, "1") == 0;
}

static int lira_io_sleep_work(void *arg, void **result) {
    int64_t millis = *(int64_t *)arg;
    struct timespec ts = {
        .tv_sec = (time_t)(millis / 1000),
        .tv_nsec = (long)((millis % 1000) * 1000000),
    };
    while (!lira_io_cancelled() && nanosleep(&ts, &ts) == -1 && errno == EINTR) {
    }
    *result = NULL;
    return 0;
}

int lira_io_submit_sleep(int64_t millis, void *owner, uint64_t generation,
                         LiraIoCompleteFn complete) {
    int64_t *arg = (int64_t *)lira_rt_mem_try_alloc(sizeof(int64_t), 0);
    if (arg == NULL) {
        return -1;
    }
    *arg = millis;
    if (lira_io_submit(lira_io_sleep_work, arg, lira_rt_mem_free, complete, NULL, owner,
                       generation) != 0) {
        lira_rt_mem_free(arg);
        return -1;
    }
    return 0;
}

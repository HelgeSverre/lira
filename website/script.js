/**
 * Lira Website Interactivity
 */

document.addEventListener('DOMContentLoaded', () => {
    initTabs();
    initCopyButtons();
    initSmoothScroll();
});

/**
 * Example tabs functionality
 */
function initTabs() {
    const tabButtons = document.querySelectorAll('.tab-btn');
    const panels = document.querySelectorAll('.example-panel');

    tabButtons.forEach(btn => {
        btn.addEventListener('click', () => {
            const tabId = btn.dataset.tab;

            // Update active states
            tabButtons.forEach(b => b.classList.remove('active'));
            panels.forEach(p => p.classList.remove('active'));

            btn.classList.add('active');
            document.getElementById(tabId)?.classList.add('active');
        });
    });
}

/**
 * Copy to clipboard functionality
 */
function initCopyButtons() {
    const copyButtons = document.querySelectorAll('.copy-btn');

    copyButtons.forEach(btn => {
        btn.addEventListener('click', async () => {
            const textToCopy = btn.dataset.copy;

            try {
                await navigator.clipboard.writeText(textToCopy);

                // Visual feedback
                btn.classList.add('copied');
                const originalIcon = btn.innerHTML;
                btn.innerHTML = `
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="16" height="16">
                        <polyline points="20 6 9 17 4 12"></polyline>
                    </svg>
                `;

                setTimeout(() => {
                    btn.classList.remove('copied');
                    btn.innerHTML = originalIcon;
                }, 2000);
            } catch (err) {
                console.error('Failed to copy:', err);
            }
        });
    });
}

/**
 * Smooth scroll for anchor links
 */
function initSmoothScroll() {
    document.querySelectorAll('a[href^="#"]').forEach(anchor => {
        anchor.addEventListener('click', function (e) {
            e.preventDefault();
            const target = document.querySelector(this.getAttribute('href'));
            if (target) {
                target.scrollIntoView({
                    behavior: 'smooth',
                    block: 'start'
                });
            }
        });
    });
}

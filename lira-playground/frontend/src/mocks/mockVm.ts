/**
 * Mock Lira Virtual Machine
 *
 * Simulates VM execution with realistic state transitions for UI development.
 * This is not a real VM - just produces plausible state updates.
 */

import type { VmState, Fiber, Channel, Value, FiberId, ChannelId } from '../types/vm';

interface MockVmCallbacks {
  onOutput: (line: string) => void;
  onStateUpdate: (state: VmState) => void;
  onFinished: (exitCode: number) => void;
  onError: (error: string) => void;
}

export class MockVm {
  private state: VmState;
  private callbacks: MockVmCallbacks;
  private running = false;
  private stepInterval: ReturnType<typeof setInterval> | null = null;
  private stepCount = 0;
  private maxSteps = 50;
  private nextFiberId: FiberId = 1;
  private nextChannelId: ChannelId = 1;

  constructor(callbacks: MockVmCallbacks) {
    this.callbacks = callbacks;
    this.state = this.createInitialState();
  }

  private createInitialState(): VmState {
    const mainFiber = this.createFiber(0);
    mainFiber.state = { type: 'Running' };

    return {
      fibers: { [mainFiber.id]: mainFiber },
      channels: {},
      currentFiberId: mainFiber.id,
      readyQueue: [],
      output: [],
      exitCode: null,
    };
  }

  private createFiber(ip: number): Fiber {
    const id = this.nextFiberId++;
    return {
      id,
      state: { type: 'Ready' },
      ip,
      stack: [],
      locals: [],
      callStack: [],
      fiberLocals: {},
      result: null,
    };
  }

  private createChannel(capacity: number): Channel {
    const id = this.nextChannelId++;
    return {
      id,
      buffer: [],
      capacity,
      receivers: [],
      senders: [],
      closed: false,
    };
  }

  run(): void {
    this.running = true;

    // Simulate execution with delays
    this.stepInterval = setInterval(() => {
      if (!this.running) {
        this.cleanup();
        return;
      }

      this.executeStep();
      this.stepCount++;

      // Check if we should finish
      if (this.stepCount >= this.maxSteps) {
        this.finish(0);
      }
    }, 100);
  }

  stop(): void {
    this.running = false;
    this.cleanup();
  }

  private cleanup(): void {
    if (this.stepInterval) {
      clearInterval(this.stepInterval);
      this.stepInterval = null;
    }
  }

  private finish(exitCode: number): void {
    this.running = false;
    this.cleanup();

    // Mark main fiber as finished
    const mainFiber = this.state.fibers[1];
    if (mainFiber) {
      mainFiber.state = { type: 'Finished' };
      mainFiber.result = { type: 'Int', value: exitCode };
    }

    this.state.exitCode = exitCode;
    this.state.currentFiberId = null;
    this.emitState();
    this.callbacks.onFinished(exitCode);
  }

  private executeStep(): void {
    // Simulate different behaviors based on step count
    const action = this.stepCount % 10;

    switch (action) {
      case 0:
        this.simulateOutput();
        break;
      case 2:
        this.simulateStackPush();
        break;
      case 3:
        this.simulateStackPop();
        break;
      case 4:
        this.simulateLocalAssign();
        break;
      case 5:
        if (this.stepCount < 20 && Object.keys(this.state.fibers).length < 4) {
          this.simulateSpawnFiber();
        }
        break;
      case 6:
        if (Object.keys(this.state.channels).length < 3) {
          this.simulateCreateChannel();
        }
        break;
      case 7:
        this.simulateChannelOp();
        break;
      case 8:
        this.simulateFiberSwitch();
        break;
      default:
        this.advanceIp();
        break;
    }

    this.emitState();
  }

  private simulateOutput(): void {
    const messages = [
      'Hello, Lira!',
      'Processing...',
      `Step ${this.stepCount}`,
      'Computing result...',
      'Value: 42',
      `Fiber #${this.state.currentFiberId} running`,
    ];
    const msg = messages[Math.floor(Math.random() * messages.length)];
    this.callbacks.onOutput(msg);
  }

  private simulateStackPush(): void {
    const currentFiber = this.getCurrentFiber();
    if (!currentFiber) return;

    const values: Value[] = [
      { type: 'Int', value: Math.floor(Math.random() * 100) },
      { type: 'Bool', value: Math.random() > 0.5 },
      { type: 'String', value: 'hello' },
      { type: 'Float', value: Math.random() * 10 },
    ];

    currentFiber.stack.push(values[Math.floor(Math.random() * values.length)]);

    // Limit stack size
    if (currentFiber.stack.length > 10) {
      currentFiber.stack = currentFiber.stack.slice(-10);
    }
  }

  private simulateStackPop(): void {
    const currentFiber = this.getCurrentFiber();
    if (!currentFiber || currentFiber.stack.length === 0) return;
    currentFiber.stack.pop();
  }

  private simulateLocalAssign(): void {
    const currentFiber = this.getCurrentFiber();
    if (!currentFiber) return;

    const slot = Math.floor(Math.random() * 5);
    const value: Value = { type: 'Int', value: Math.floor(Math.random() * 100) };

    // Ensure locals array is big enough
    while (currentFiber.locals.length <= slot) {
      currentFiber.locals.push({ type: 'Null' });
    }
    currentFiber.locals[slot] = value;
  }

  private simulateSpawnFiber(): void {
    const newFiber = this.createFiber(100 + this.nextFiberId * 10);
    newFiber.state = { type: 'Ready' };
    this.state.fibers[newFiber.id] = newFiber;
    this.state.readyQueue.push(newFiber.id);
    this.callbacks.onOutput(`Spawned fiber #${newFiber.id}`);
  }

  private simulateCreateChannel(): void {
    const capacity = Math.floor(Math.random() * 5) + 1;
    const channel = this.createChannel(capacity);
    this.state.channels[channel.id] = channel;
    this.callbacks.onOutput(`Created channel #${channel.id} (capacity: ${capacity})`);
  }

  private simulateChannelOp(): void {
    const channelIds = Object.keys(this.state.channels).map(Number);
    if (channelIds.length === 0) return;

    const channelId = channelIds[Math.floor(Math.random() * channelIds.length)];
    const channel = this.state.channels[channelId];
    if (!channel || channel.closed) return;

    if (Math.random() > 0.5 && channel.buffer.length < channel.capacity) {
      // Send
      const value: Value = { type: 'Int', value: Math.floor(Math.random() * 100) };
      channel.buffer.push(value);
      this.callbacks.onOutput(`Sent to channel #${channelId}`);
    } else if (channel.buffer.length > 0) {
      // Receive
      const value = channel.buffer.shift();
      this.callbacks.onOutput(`Received from channel #${channelId}`);

      // Put value on current fiber's stack
      const currentFiber = this.getCurrentFiber();
      if (currentFiber && value) {
        currentFiber.stack.push(value);
      }
    }
  }

  private simulateFiberSwitch(): void {
    const fiberIds = Object.keys(this.state.fibers).map(Number);
    if (fiberIds.length <= 1) return;

    // Find a ready fiber to switch to
    const readyFibers = fiberIds.filter((id) => {
      const fiber = this.state.fibers[id];
      return fiber && (fiber.state.type === 'Ready' || fiber.state.type === 'Yielded');
    });

    if (readyFibers.length === 0) return;

    // Mark current fiber as yielded
    const currentFiber = this.getCurrentFiber();
    if (currentFiber) {
      currentFiber.state = { type: 'Yielded' };
    }

    // Switch to new fiber
    const newFiberId = readyFibers[Math.floor(Math.random() * readyFibers.length)];
    const newFiber = this.state.fibers[newFiberId];
    if (newFiber) {
      newFiber.state = { type: 'Running' };
      this.state.currentFiberId = newFiberId;
    }
  }

  private advanceIp(): void {
    const currentFiber = this.getCurrentFiber();
    if (currentFiber) {
      currentFiber.ip++;
    }
  }

  private getCurrentFiber(): Fiber | null {
    if (this.state.currentFiberId === null) return null;
    return this.state.fibers[this.state.currentFiberId] ?? null;
  }

  private emitState(): void {
    // Create a deep copy of the state
    const stateCopy: VmState = JSON.parse(JSON.stringify(this.state));
    this.callbacks.onStateUpdate(stateCopy);
  }
}

// The parts of node's test runner the UI tests use; the project has no @types/node.

declare module "node:test" {
  export function test(name: string, fn: () => void | Promise<void>): void;
  export function beforeEach(fn: () => void | Promise<void>): void;
  export const mock: {
    timers: {
      enable(options?: { apis?: ("setTimeout" | "setInterval" | "Date")[]; now?: number }): void;
      reset(): void;
      tick(ms: number): void;
    };
  };
}

declare module "node:assert/strict" {
  const assert: {
    equal(actual: unknown, expected: unknown, message?: string): void;
    deepEqual(actual: unknown, expected: unknown, message?: string): void;
    ok(value: unknown, message?: string): void;
  };
  export default assert;
}

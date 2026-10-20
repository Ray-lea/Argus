export function validTaskCommand(command: string): boolean {
  return command.trim().length > 0 && command.length <= 8192 && !/[\0\r\n]/.test(command);
}

export function quotedTaskPath(path: string): string {
  return `"${path}"`;
}

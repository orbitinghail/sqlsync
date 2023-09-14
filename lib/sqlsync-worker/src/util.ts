export function assertUnreachable(err: string, x: never): never {
  throw new Error(`unreachable: ${err}; got ${JSON.stringify(x)}`);
}

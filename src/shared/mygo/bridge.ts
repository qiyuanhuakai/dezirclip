declare global {
  interface Window {
    mygo: {
      call<T>(method: string, ...args: unknown[]): Promise<T>;
      on<T>(name: string, callback: (payload: T) => void): () => void;
    };
  }
}

export function shellCall<T>(method: string, ...args: unknown[]): Promise<T> {
  return window.mygo.call<T>(`DesktopShell.${method}`, ...args);
}

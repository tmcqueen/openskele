// Thin runtime for openskele plugins: JSON-RPC 2.0 over stdio with
// Content-Length framing, plus a small typed surface over the protocol.
// The protocol types are generated; this file is the only hand-written part.

import process from "node:process";

import {
  PROTOCOL_VERSION,
  type ConfigDidChangeParams,
  type DocumentDidSaveParams,
  type HostNotifications,
  type HostRequests,
  type MessageLevel,
  type PluginNotifications,
  type PluginRequests,
  type ToolCallResult,
  type TreeItem,
} from "./generated/protocol";

export * from "./generated/protocol";

type Awaitable<T> = T | Promise<T>;

export interface TreeDataProvider {
  /** Rows of the view, or the children of the row with id `parent`. */
  getChildren(parent?: string): Awaitable<TreeItem[]>;
}

export interface Host {
  readonly pluginId: string;
  readonly workspaceFolder?: string;
  readonly commands: {
    /** Implements a command declared in the manifest. */
    register(id: string, handler: (...args: unknown[]) => unknown): void;
  };
  readonly tools: {
    /**
     * Implements a tool declared in the manifest. The handler gets the
     * arguments, already checked against the tool's `inputSchema`. Return
     * text, or a full result; a thrown error becomes a result with `isError`.
     */
    register(name: string, handler: (args: Record<string, unknown>) => Awaitable<ToolCallResult | string>): void;
  };
  readonly views: {
    /** Supplies the data for a view declared in the manifest. */
    registerTreeDataProvider(viewId: string, provider: TreeDataProvider): void;
    /** Tells the host the view's data changed so it queries again. */
    refresh(viewId: string): void;
  };
  readonly window: {
    showMessage(message: string, level?: MessageLevel): void;
  };
  readonly terminal: {
    /** Appends to an output terminal; defaults to the plugin's own. */
    write(data: string, terminal?: string): void;
  };
  readonly config: {
    /** Effective value of a setting: workspace, then user, then default. */
    get<T = unknown>(key: string): Promise<T>;
    onDidChange(listener: (changes: ConfigDidChangeParams["changes"]) => void): void;
  };
  readonly documents: {
    onDidSave(listener: (event: DocumentDidSaveParams) => void): void;
  };
}

export interface PluginDefinition {
  name: string;
  version: string;
  /** Runs once, before the host is told the plugin is ready. Register handlers here. */
  activate(host: Host): Awaitable<void>;
  deactivate?(): Awaitable<void>;
}

class RpcError extends Error {
  constructor(
    readonly code: number,
    message: string,
  ) {
    super(message);
  }
}

const METHOD_NOT_FOUND = -32601;
const INTERNAL_ERROR = -32603;

/** Connects to the host over stdio and serves the plugin until the host exits. */
export function runPlugin(definition: PluginDefinition): void {
  // stdout belongs to the protocol; keep stray logging off it.
  console.log = console.error;
  console.info = console.error;

  const commands = new Map<string, (...args: unknown[]) => unknown>();
  const providers = new Map<string, TreeDataProvider>();
  const tools = new Map<string, (args: Record<string, unknown>) => Awaitable<ToolCallResult | string>>();
  const configListeners: ((changes: ConfigDidChangeParams["changes"]) => void)[] = [];
  const saveListeners: ((event: DocumentDidSaveParams) => void)[] = [];
  const pending = new Map<number, { resolve(value: unknown): void; reject(error: Error): void }>();
  let nextId = 1;

  function send(message: object): void {
    const body = Buffer.from(JSON.stringify({ jsonrpc: "2.0", ...message }), "utf8");
    process.stdout.write(`Content-Length: ${body.length}\r\n\r\n`);
    process.stdout.write(body);
  }

  function notify<M extends keyof PluginNotifications>(method: M, params: PluginNotifications[M]): void {
    send({ method, params });
  }

  function request<M extends keyof PluginRequests>(
    method: M,
    params: PluginRequests[M]["params"],
  ): Promise<PluginRequests[M]["result"]> {
    const id = nextId++;
    send({ id, method, params });
    return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
  }

  const requests: { [M in keyof HostRequests]: (params: HostRequests[M]["params"]) => Awaitable<HostRequests[M]["result"]> } = {
    async initialize(params) {
      const host: Host = {
        pluginId: params.pluginId,
        workspaceFolder: params.workspaceFolder,
        commands: { register: (id, handler) => void commands.set(id, handler) },
        tools: { register: (name, handler) => void tools.set(name, handler) },
        views: {
          registerTreeDataProvider: (viewId, provider) => void providers.set(viewId, provider),
          refresh: (viewId) => notify("view/refresh", { viewId }),
        },
        window: { showMessage: (message, level = "info") => notify("window/showMessage", { level, message }) },
        terminal: { write: (data, terminal) => notify("terminal/write", { data, terminal }) },
        config: {
          get: (key) => request("config/get", { key }) as Promise<never>,
          onDidChange: (listener) => void configListeners.push(listener),
        },
        documents: { onDidSave: (listener) => void saveListeners.push(listener) },
      };
      await definition.activate(host);
      return {
        protocolVersion: PROTOCOL_VERSION,
        plugin: { name: definition.name, version: definition.version },
      };
    },
    async shutdown() {
      await definition.deactivate?.();
      return {};
    },
    "command/execute"({ command, args = [] }) {
      const handler = commands.get(command);
      if (!handler) throw new RpcError(METHOD_NOT_FOUND, `no handler registered for command ${command}`);
      // JSON-RPC has no `undefined`; a command without a return value yields null.
      return Promise.resolve(handler(...args)).then((value) => value ?? null);
    },
    async "tool/call"({ name, arguments: args }) {
      const handler = tools.get(name);
      if (!handler) throw new RpcError(METHOD_NOT_FOUND, `no handler registered for tool ${name}`);
      try {
        const result = await handler((args ?? {}) as Record<string, unknown>);
        return typeof result === "string" ? { content: [{ type: "text", text: result }], isError: false } : result;
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        return { content: [{ type: "text", text: message }], isError: true };
      }
    },
    async "view/getChildren"({ viewId, parent }) {
      const provider = providers.get(viewId);
      if (!provider) throw new RpcError(METHOD_NOT_FOUND, `no data provider registered for view ${viewId}`);
      return { items: await provider.getChildren(parent) };
    },
  };

  const notifications: { [M in keyof HostNotifications]: (params: HostNotifications[M]) => void } = {
    exit: () => process.exit(0),
    "config/didChange": ({ changes }) => configListeners.forEach((listener) => listener(changes)),
    "document/didSave": (event) => saveListeners.forEach((listener) => listener(event)),
  };

  async function dispatch(message: any): Promise<void> {
    if (typeof message.method !== "string") {
      const waiter = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) waiter?.reject(new RpcError(message.error.code, message.error.message));
      else waiter?.resolve(message.result);
      return;
    }
    if (message.id === undefined) {
      try {
        (notifications as Record<string, ((params: unknown) => void) | undefined>)[message.method]?.(message.params ?? {});
      } catch (error) {
        console.error(`${message.method} listener failed:`, error);
      }
      return;
    }
    try {
      const handler = (requests as Record<string, ((params: unknown) => unknown) | undefined>)[message.method];
      if (!handler) throw new RpcError(METHOD_NOT_FOUND, `method not found: ${message.method}`);
      send({ id: message.id, result: await handler(message.params ?? {}) });
    } catch (error) {
      const code = error instanceof RpcError ? error.code : INTERNAL_ERROR;
      send({ id: message.id, error: { code, message: error instanceof Error ? error.message : String(error) } });
    }
  }

  let buffer: Buffer = Buffer.alloc(0);
  process.stdin.on("data", (chunk: Buffer) => {
    buffer = Buffer.concat([buffer, chunk]);
    for (;;) {
      const headerEnd = buffer.indexOf("\r\n\r\n");
      if (headerEnd < 0) return;
      const match = /content-length:\s*(\d+)/i.exec(buffer.subarray(0, headerEnd).toString("ascii"));
      if (!match) {
        // Not a frame we understand; skip past it rather than stall.
        buffer = buffer.subarray(headerEnd + 4);
        continue;
      }
      const start = headerEnd + 4;
      const end = start + Number(match[1]);
      if (buffer.length < end) return;
      const body = buffer.subarray(start, end).toString("utf8");
      buffer = buffer.subarray(end);
      void dispatch(JSON.parse(body));
    }
  });
  // The host going away is the signal to stop, with or without `exit`.
  process.stdin.on("end", () => process.exit(0));
}

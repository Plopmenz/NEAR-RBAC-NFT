function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunkSize = 0x8000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunkSize));
  }
  return btoa(binary);
}

export class RpcError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "RpcError";
  }
}

async function rpcRequest<T>(rpcUrl: string, body: unknown): Promise<T> {
  const response = await fetch(rpcUrl, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(20_000),
  });
  if (!response.ok) {
    throw new RpcError(`RPC returned HTTP ${response.status}`);
  }
  const payload = (await response.json()) as {
    result?: unknown;
    error?: { message?: string; data?: unknown };
  };
  if (payload.error) {
    throw new RpcError(payload.error.message || "RPC request failed");
  }
  return payload.result as T;
}

/**
 * Calls a view method through NEAR JSON-RPC and decodes the JSON result.
 *
 * Contract panics are returned inside `result.error`, so they are surfaced as `RpcError` too.
 */
export async function viewFunction<T>(
  rpcUrl: string,
  contractId: string,
  methodName: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  const argsBase64 = bytesToBase64(new TextEncoder().encode(JSON.stringify(args)));
  const result = await rpcRequest<{
    result?: number[];
    error?: string;
  }>(rpcUrl, {
    jsonrpc: "2.0",
    id: `${contractId}.${methodName}`,
    method: "query",
    params: {
      request_type: "call_function",
      finality: "final",
      account_id: contractId,
      method_name: methodName,
      args_base64: argsBase64,
    },
  });

  if (result.error) {
    throw new RpcError(result.error);
  }
  if (!Array.isArray(result.result)) {
    throw new RpcError("Unexpected RPC result shape");
  }
  const text = new TextDecoder().decode(Uint8Array.from(result.result));
  try {
    return JSON.parse(text) as T;
  } catch {
    throw new RpcError("Contract returned invalid JSON");
  }
}

/** Returns the latest final block timestamp in nanoseconds. */
export async function getFinalBlockTimestampNs(rpcUrl: string): Promise<bigint> {
  const result = await rpcRequest<{ header?: { timestamp?: number } }>(rpcUrl, {
    jsonrpc: "2.0",
    id: "block",
    method: "block",
    params: { finality: "final" },
  });
  const timestamp = result.header?.timestamp;
  if (typeof timestamp !== "number" || !Number.isFinite(timestamp)) {
    throw new RpcError("RPC did not return a block timestamp");
  }
  return BigInt(timestamp);
}

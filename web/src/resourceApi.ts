import { hostJson } from "./host";

export async function postResourceApi<T>(url: string, body: unknown): Promise<T> {
  return hostJson<T>(url, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
}

export const MAX_CLIENT_RESOURCE_BYTES = 512 * 1024 * 1024;

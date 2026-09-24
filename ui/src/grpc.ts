import { inject, type Plugin } from "vue";
import { createClient, type Client } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { AdminService } from "./api/ora/admin/v1/admin_pb";

const clientSymbol = Symbol("gRPC Client");

export interface OraAdminClientOptions {
  /**
   * Whether to send credentials (e.g. cookies) with the requests,
   * `"include"` is required for cross-origin APIs behind authentication.
   */
  credentials?: RequestCredentials;
}

export function oraAdminClient(
  url: string,
  { credentials }: OraAdminClientOptions = {},
): Plugin {
  return {
    install(app) {
      const transport = createGrpcWebTransport({
        baseUrl: url,
        fetch: (input, init) => globalThis.fetch(input, { ...init, credentials }),
      });

      app.provide(clientSymbol, createClient(AdminService, transport));
    },
  };
}

export function useOraAdminClient(): Client<typeof AdminService> {
  const svc = inject<Client<typeof AdminService>>(clientSymbol);

  if (!svc) {
    throw new Error("Ora Admin Client not provided");
  }

  return svc;
}

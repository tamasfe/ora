import { inject, type Plugin } from "vue";
import { createClient, type Client } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { AdminService } from "./api/ora/admin/v1/admin_pb";

const clientSymbol = Symbol("gRPC Client");

export function oraAdminClient(url: string): Plugin {
  return {
    install(app) {
      const transport = createGrpcWebTransport({
        baseUrl: url,
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

/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Base URL of the Ora gRPC-web API. */
  readonly VITE_ORA_API_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

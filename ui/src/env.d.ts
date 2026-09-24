/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Base URL of the Ora gRPC-web API. */
  readonly VITE_ORA_API_URL?: string;
  /** Set to `include` to send credentials (e.g. cookies) to the API. */
  readonly VITE_ORA_API_CREDENTIALS?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

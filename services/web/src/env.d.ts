/// <reference types="astro/client" />

interface ImportMetaEnv {
  readonly PUBLIC_NEAR_NETWORK?: string;
  readonly PUBLIC_NEAR_RPC_URL?: string;
  readonly PUBLIC_FACTORY_CONTRACT_ID?: string;
  readonly PUBLIC_EXPECTED_CHILD_CODE_HASH?: string;
  readonly PUBLIC_EXPLORER_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}

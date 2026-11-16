import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";

export interface Provider {
  id: string;
  name: string;
  base_url: string;
  models: string[];
  /** Whether an API key is stored in the Windows Credential Manager. The key itself never reaches the screen. */
  has_key: boolean;
}

export interface ModelsView {
  providers: Provider[];
  active: { provider: string; model: string } | null;
  /** No model is chosen and the LLM_* environment variables are in use. */
  env_fallback: boolean;
}

export const saveProvider = (id: string | null, name: string, baseUrl: string, apiKey: string) =>
  invoke<ModelsView>("save_provider", { id, name, baseUrl, apiKey });
export const removeProvider = (id: string) => invoke<ModelsView>("remove_provider", { id });
export const addModel = (id: string, model: string) => invoke<ModelsView>("add_model", { id, model });
export const removeModel = (id: string, model: string) => invoke<ModelsView>("remove_model", { id, model });
export const activateModel = (id: string, model: string) => invoke<ModelsView>("activate_model", { id, model });
export const fetchModels = (id: string) => invoke<string[]>("fetch_models", { id });
export const testModel = (id: string, model: string) => invoke<string>("test_model", { id, model });

/** The providers and the chosen model, shared by the header picker and the Model screen. */
export function useModels() {
  const [models, setModels] = useState<ModelsView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<ModelsView>("get_models").then(setModels, (reason) => setError(String(reason)));
  }, []);

  /** Runs a change and shows what it returns. */
  const apply = useCallback(async (change: Promise<ModelsView>) => {
    try {
      setModels(await change);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }, []);

  return { models, error, apply };
}

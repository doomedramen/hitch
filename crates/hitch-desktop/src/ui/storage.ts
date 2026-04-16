import { Store } from "@tauri-apps/plugin-store";
import { RepoEntry } from "./types";

const STORE_FILE = "hitch-desktop.json";
const KEY = "repos_v1";

let storePromise: Promise<Store> | null = null;

async function getStore(): Promise<Store> {
  if (!storePromise) {
    storePromise = Store.load(STORE_FILE);
  }
  return storePromise;
}

export async function loadRepos(): Promise<RepoEntry[]> {
  try {
    const store = await getStore();
    const raw = (await store.get(KEY)) as unknown;
    if (!Array.isArray(raw)) return [];
    return raw as RepoEntry[];
  } catch {
    return [];
  }
}

export async function saveRepos(repos: RepoEntry[]): Promise<void> {
  const store = await getStore();
  await store.set(KEY, repos);
  await store.save();
}

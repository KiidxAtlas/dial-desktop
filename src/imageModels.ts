export interface RoleModel {
  provider: string;
  provider_label?: string;
  model_id: string;
  label: string;
  base_url?: string | null;
  image_api?: unknown;
  image_capability?: "capable" | "candidate" | "incapable";
  image_capability_source?: "verified" | "metadata" | "documented" | "candidate";
  image_checked_at?: number;
  enabled?: boolean;
  current?: boolean;
  output_modalities?: string[];
}

export interface ImageModelsChecked {
  providers: number;
  capable: number;
  candidates: number;
  checked_at?: number;
}

export interface ImageRoute {
  ok: boolean;
  model?: string;
  api?: string;
  role: { provider: string; model: string } | null;
}

/** An assigned image role identifies its provider explicitly. For an automatic
 * route, only the active catalog row identifies the conversation model. */
export function matchesImageRoute(row: RoleModel, route?: ImageRoute): boolean {
  if (!route?.ok || !route.model || row.model_id !== route.model) return false;
  return route.role
    ? route.role.provider === row.provider && route.role.model === row.model_id
    : row.current === true;
}

/** Display label for an image model option, annotating verified vs unverified candidates. */
export function imageModelLabel(row: RoleModel, fallback?: string): string {
  const base = row.label || fallback || row.model_id;
  if (row.image_capability_source === "verified") return `${base} · verified`;
  if (row.image_capability === "candidate") return `${base} · unverified`;
  return base;
}

export function imageChoices(image: RoleModel[], catalog: RoleModel[], route?: ImageRoute): RoleModel[] {
  const choices = new Map<string, RoleModel>();
  function add(row: RoleModel) {
    const key = `${row.provider}\u0000${row.model_id}`;
    if (
      row.enabled !== false &&
      row.image_capability !== "incapable" &&
      row.provider &&
      row.model_id &&
      !choices.has(key)
    ) {
      choices.set(key, row);
    }
  }
  // Local generators and cached candidates in the image listing have image_api: null.
  // Older engines return chat rows instead; vision input is not image output.
  for (const row of image) {
    if (
      "image_api" in row ||
      row.output_modalities?.includes("image") ||
      row.image_capability === "capable" ||
      row.image_capability === "candidate"
    ) {
      add(row);
    }
  }
  for (const row of catalog) {
    if (row.image_api || row.output_modalities?.includes("image") || matchesImageRoute(row, route)) add(row);
  }
  return [...choices.values()];
}

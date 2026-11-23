/** The engine validates interval syntax, watch patterns and ids when it saves. */
export type ScheduleDraft = {
  prompt: string;
  id?: string;
  every?: string;
  watch?: string[];
  allow?: string[];
  approve_all?: boolean;
}

/** Parse the engine's `/schedule add` flags, stopping at the first prompt word. */
export function typedSchedule(words: string[]): ScheduleDraft {
  const draft: ScheduleDraft = { prompt: "" };
  for (let i = 0; i < words.length; i++) {
    const flag = words[i];
    if (flag === "--yes" || flag === "-y") draft.approve_all = true;
    else if (["--every", "--id", "--watch", "--allow"].includes(flag)) {
      const value = words[++i];
      if (!value) throw new Error(`${flag} needs a value`);
      if (flag === "--every") draft.every = value;
      else if (flag === "--id") draft.id = value;
      else if (flag === "--watch") (draft.watch ??= []).push(value);
      else (draft.allow ??= []).push(value);
    } else {
      draft.prompt = words.slice(flag === "--" ? i + 1 : i).join(" ");
      break;
    }
  }
  return draft;
}

export async function saveSchedule(
  draft: ScheduleDraft, root: string, ask: (summary: string) => Promise<boolean>,
  save: (request: ScheduleDraft & { confirm: true }) => Promise<unknown>,
) {
  if (!draft.prompt.trim() || (!draft.every && !draft.watch?.length)) throw new Error("Enter a prompt and an interval or file pattern");
  const access = draft.approve_all ? "ALL tools pre-approved" : draft.allow?.length ? `Read-only plus ${draft.allow.join(", ")}` : "Read-only";
  const summary = [`Save this schedule for ${root}?`, draft.prompt, draft.every && `Every ${draft.every}`, draft.watch?.length && `When files change: ${draft.watch.join(", ")}`, `Access: ${access}`, "A running scheduler may pick it up immediately."].filter(Boolean).join("\n\n");
  if (!(await ask(summary))) return false;
  await save({ ...draft, confirm: true });
  return true;
}

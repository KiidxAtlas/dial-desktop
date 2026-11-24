// How the window's width is shared between the sidebar, the conversation and the inspector.
// Pure, so it can be tested without the app.

/** Narrowest the inspector gets. */
export const MIN_WIDTH = 260;
/** Room the conversation keeps beside the inspector (and the sidebar's, while it shows). */
export const CONVERSATION_MIN = 540;
/** The sidebar's width plus its margin. */
export const SIDEBAR = 280;

/** Whether the sidebar fits beside an open inspector in a window `width` CSS pixels wide.
 *  When it does not, the sidebar steps aside while the inspector is open. */
export function sidebarFits(width: number, inspectorOpen: boolean): boolean {
  return !inspectorOpen || width >= SIDEBAR + CONVERSATION_MIN + MIN_WIDTH;
}

/** The widest the inspector may get, leaving the conversation (and a shown sidebar) their room. */
export function maxInspectorWidth(width: number, sidebarShown: boolean): number {
  return Math.max(MIN_WIDTH, width - CONVERSATION_MIN - (sidebarShown ? SIDEBAR : 0));
}

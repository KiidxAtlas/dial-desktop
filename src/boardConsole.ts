// Pure helpers for the Boards panel: naming a serial device, and turning bytes into what the
// console shows or sends.

export type Board = {
  path: string;
  transport: 'usb' | 'bluetooth' | 'builtin' | 'other';
  product: string | null;
  manufacturer: string | null;
  serial_number: string | null;
  usb_id: string | null;
  kind: string | null;
  hint: string | null;
};

export const ENDINGS = { lf: '\n', crlf: '\r\n', cr: '\r', none: '' } as const;
export type Ending = keyof typeof ENDINGS;

const TRANSPORT = { usb: 'USB serial device', bluetooth: 'Bluetooth serial port', builtin: 'Built-in serial port', other: 'Serial port' };

/** The most recognisable name available: what the device calls itself, else our best guess. */
export function boardName(board: Board) {
  return board.product?.trim() || board.kind || TRANSPORT[board.transport];
}

/** Everything else known about the device, most identifying first. */
export function boardDetails(board: Board) {
  const name = boardName(board);
  return [
    board.kind !== name ? board.kind : null,
    board.manufacturer?.trim(),
    TRANSPORT[board.transport] !== name && board.transport !== 'usb' ? TRANSPORT[board.transport] : null,
    board.usb_id && `USB ${board.usb_id}`,
    board.serial_number && `S/N ${board.serial_number}`,
  ].filter(Boolean).join(' · ');
}

/** Bytes written as hex (`DE AD`, `0xDE,0xAD`, `dead`); `null` when the text is not hex bytes. */
export function parseHex(text: string): Uint8Array | null {
  const bytes: number[] = [];
  for (const token of text.replace(/0x/gi, ' ').split(/[\s,:;-]+/).filter(Boolean)) {
    if (!/^[0-9a-f]+$/i.test(token) || (token.length > 1 && token.length % 2)) return null;
    for (const pair of token.match(/.{1,2}/g)!) bytes.push(parseInt(pair, 16));
  }
  return bytes.length ? Uint8Array.from(bytes) : null;
}

/** A hex dump for the console: 16 bytes a row with their printable characters alongside. */
export function hexRows(bytes: Uint8Array) {
  let out = '';
  for (let i = 0; i < bytes.length; i += 16) {
    const row = [...bytes.subarray(i, i + 16)];
    const hex = row.map(b => b.toString(16).padStart(2, '0')).join(' ');
    const text = row.map(b => (b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : '.')).join('');
    out += `${hex.padEnd(47)}  ${text}\r\n`;
  }
  return out;
}

/** Console output as plain log lines: no colour codes, carriage returns or control characters. */
export function cleanLog(text: string) {
  return text
    .replace(/\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)?|[@-Z\\-_])/g, '')
    .replace(/\r\n?/g, '\n')
    .replace(/[\x00-\x08\x0b-\x1f\x7f]/g, '');
}

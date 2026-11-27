export type UsbItem = { product: string | null; kind: string; usb_id: string; location: string; functions: string[]; speed: string | null; interfaces: string[] };
export type Battery = { percent: number | null; usb_powered: boolean | null; voltage_mv: number | null; temperature_c: number | null };

export function usbReport(i: UsbItem): string {
  return [`${i.product || i.kind} (${i.kind})`, `USB ${i.usb_id} · Port ${i.location}`,
    `Link: ${i.speed ?? 'not reported'}`, `Functions: ${i.functions.join(', ') || 'none recognized'}`,
    `Interfaces (class/subclass/protocol): ${i.interfaces.join('; ') || 'not reported'}`].join('\n');
}

export function filterUsb(items: UsbItem[], query: string): UsbItem[] {
  const words = query.trim().toLowerCase().split(/\s+/);
  return items.filter(i => { const text = usbReport(i).toLowerCase(); return words.every(w => text.includes(w)); });
}

/** Compare connections, not device identities. No serial numbers or persistent history. */
export function compareUsb(before: UsbItem[], after: UsbItem[]): string[] {
  const changes: string[] = [];
  const previous = new Map(before.map(item => [item.location, item]));
  const describe = (i: UsbItem) => `${i.product || i.kind} (${i.usb_id})`;
  for (const item of after) {
    const old = previous.get(item.location);
    previous.delete(item.location);
    if (!old) changes.push(`Added at ${item.location}: ${describe(item)}`);
    else {
      const details = [];
      if (old.usb_id !== item.usb_id || old.product !== item.product) details.push(`${describe(old)} → ${describe(item)}`);
      if (old.kind !== item.kind) details.push(`${old.kind} → ${item.kind}`);
      if (old.speed !== item.speed) details.push(`link ${old.speed ?? 'unknown'} → ${item.speed ?? 'unknown'}`);
      for (const f of item.functions.filter(f => !old.functions.includes(f))) details.push(`added ${f}`);
      for (const f of old.functions.filter(f => !item.functions.includes(f))) details.push(`removed ${f}`);
      if ([...old.interfaces].sort().join(';') !== [...item.interfaces].sort().join(';')) details.push('interface descriptors changed');
      if (details.length) changes.push(`Changed at ${item.location}: ${details.join('; ')}`);
    }
  }
  for (const old of previous.values()) changes.push(`Removed from ${old.location}: ${describe(old)}`);
  return changes;
}

export function batteryText(b: Battery): string {
  return [b.percent !== null && `${b.percent}% charge`, b.usb_powered !== null && `USB power ${b.usb_powered ? 'on' : 'off'}`, b.temperature_c !== null && `${b.temperature_c} °C`, b.voltage_mv !== null && `${b.voltage_mv} mV`].filter(Boolean).join(' · ');
}

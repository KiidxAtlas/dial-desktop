import type { DownloadStatus } from './bridge';

/** Hub file sizes and helper output use decimal units. */
export const formatModelBytes = (bytes: number) => bytes >= 1e9 ? `${(bytes / 1e9).toFixed(2)} GB` : bytes >= 1e6 ? `${Math.round(bytes / 1e6)} MB` : bytes >= 1e3 ? `${Math.round(bytes / 1e3)} kB` : `${bytes} B`;

/** Received (wire) and processed (logical, may include cached data) are distinct counters.
 * Neither divided by the selection estimate tells us what fraction has finished. */
export function downloadProgress(status: DownloadStatus | null) {
  if (!status?.running) return null;
  const { percent, completedBytes, totalBytes, receivedBytes, completedFiles, totalFiles } = status;
  const filesKnown = completedFiles != null && totalFiles != null && totalFiles > 0;
  const parts: string[] = [];
  if (percent != null) parts.push(`${percent}%`);
  if (completedBytes != null) {
    const total = totalBytes != null ? ` of ${formatModelBytes(totalBytes)}` : '';
    parts.push(`${formatModelBytes(completedBytes)}${total} processed`);
  }
  if (receivedBytes != null) parts.push(`${formatModelBytes(receivedBytes)} received`);
  if (filesKnown) parts.push(`${completedFiles} of ${totalFiles} ${totalFiles === 1 ? 'file' : 'files'} ready`);
  const countFiles = percent == null && filesKnown;
  return {
    label: countFiles ? 'Model files ready' : 'Model download progress',
    value: percent ?? (countFiles ? completedFiles : undefined),
    max: countFiles ? totalFiles : 100,
    detail: parts.length ? parts.join(' · ') : 'Waiting for the download helper to report progress…',
  };
}

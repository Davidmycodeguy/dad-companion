const whole = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 });

/** Gold as the game shows it: whole coins with thousands separators ("12,400"). */
export function formatGold(value: number): string {
  return whole.format(Math.round(value));
}

/** "just now", "5 min ago", "3 h ago", "2 d ago" for an age in seconds. */
export function formatAge(seconds: number): string {
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
  return `${Math.floor(seconds / 86400)} d ago`;
}

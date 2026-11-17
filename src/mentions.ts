/** The names the user answers to, from a comma separated list such as "Budi, Pak Budi". */
export function parseNames(list: string): string[] {
  return list
    .split(",")
    .map((name) => name.trim())
    .filter((name) => name.length >= 2);
}

/** Whether `text` says one of `names` as a whole word, ignoring case ("Budi" is not in "Budiman"). */
export function mentions(text: string, names: string[]): boolean {
  return names.some((name) => {
    const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    return new RegExp(`(?<![\\p{L}\\p{N}])${escaped}(?![\\p{L}\\p{N}])`, "iu").test(text);
  });
}

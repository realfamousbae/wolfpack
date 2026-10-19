// Slugs for URLs: lower case letters and digits, with one dash between words.

export function slugify(title: string): string {
  return title
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export function countWords(title: string): number {
  return (title.match(/[A-Za-z0-9]+/g) ?? []).length;
}

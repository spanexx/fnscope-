// String helpers convert free-form text into stable, display-ready values.

// Slugify turns arbitrary text into a lowercase, hyphen-separated URL slug.
export const slugify = (value: string): string =>
  value
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '');

// Formatter prepares raw strings for display in the explorer UI.
export class Formatter {
  // Format collapses whitespace and capitalizes each word in the value.
  format(value: string): string {
    const words = value.trim().split(/\s+/).filter(Boolean);
    return words
      .map((word) => word.charAt(0).toUpperCase() + word.slice(1).toLowerCase())
      .join(' ');
  }
}

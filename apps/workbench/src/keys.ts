// Keyboard chords in the registry's notation: `Mod+Shift+P`, where `Mod` is
// Cmd on macOS and Ctrl elsewhere.

const isMac = navigator.platform.toLowerCase().includes("mac");
const MODIFIERS = ["Mod", "Ctrl", "Alt", "Shift"];

function canonical(parts: string[]): string {
  const key = parts.filter((part) => !MODIFIERS.includes(part)).pop() ?? "";
  const modifiers = MODIFIERS.filter((modifier) => parts.includes(modifier));
  return [...modifiers, key.length === 1 ? key.toUpperCase() : key].join("+");
}

export function normalizeChord(chord: string): string {
  return canonical(chord.split("+").map((part) => part.trim()));
}

export function chordOf(event: KeyboardEvent): string | null {
  if (["Control", "Meta", "Alt", "Shift"].includes(event.key)) return null;
  const parts: string[] = [];
  if (isMac ? event.metaKey : event.ctrlKey) parts.push("Mod");
  if (isMac && event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  // `code` is layout-position based, so Alt/Shift do not change what a letter or digit reports.
  const code = /^(?:Key|Digit)(.)$/.exec(event.code);
  parts.push(code ? code[1] : event.key);
  return canonical(parts);
}

export function displayChord(chord: string): string {
  return chord.replace("Mod", isMac ? "⌘" : "Ctrl");
}

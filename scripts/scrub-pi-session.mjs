#!/usr/bin/env node
// Strips all personal data from a pi agent session (a JSONL file) for the
// `sessions` benchmark.
//
// Usage: node scripts/scrub-pi-session.mjs INPUT OUTPUT START
//
// The structure of the session is kept: every entry, key, number, boolean
// and null is where it was, and the lengths of all strings stay about the
// same.  Only values that identify nothing are kept verbatim: the tags
// (`type`, `role`), providers, models, APIs, tool names, stop reasons and
// the names of parameters in tool schemas.  Everything else is replaced:
//
// * Text (prompts, answers, thinking, tool calls and output, paths, error
//   messages, the system prompt, signatures): every run of ASCII letters is
//   replaced by random words of the same length and case, every run of
//   digits by random digits and every non-ASCII letter by a random letter
//   with the same UTF-8 length.  Punctuation, whitespace and control
//   characters stay, so the JSON escapes are the same as in the original.
// * Ids are replaced by random ids of the same shape.  The same id is
//   replaced by the same random id so that the entries still link up.
// * Timestamps are shifted so that the session starts at START.
// * Images are replaced by generated PNGs of about the same size.
//
// The replacements are random and do not depend on the replaced text (only
// on its length), so nothing of the text can be recovered.  The used random
// numbers are seeded to make runs reproducible.
//
// Prints the values that were kept verbatim and all keys, to check them.
import fs from "node:fs";
import zlib from "node:zlib";

const [input, output, start] = process.argv.slice(2);
if (!input || !output || !start) {
  console.error("usage: scrub-pi-session.mjs INPUT OUTPUT START");
  process.exit(1);
}

// mulberry32
let seed = 0x5eed1e55;
function random() {
  seed = (seed + 0x6d2b79f5) | 0;
  let t = seed;
  t = Math.imul(t ^ (t >>> 15), t | 1);
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
}
const pick = (items) => items[Math.floor(random() * items.length)];

const WORDS = `
a an as at be by do go he if in is it me my no of on or so to up us we
add all and any are box but can day end few for get has how its let lot
may new not now off old one our out put run say see set the too two use
way who why yes you also back been best both call came come data does
done each even file find from full give good have here high into just
keep kind last left life like line list long look made make many more
most move much must name need next note only open over part path plan
read real rest same seem show side some such take tell test text than
that them then they this time tree turn type unit used very want well
what when will with word work year about above after again along being
below build check class clear close could every field first found given
group heavy house large later light might never order other place point
right round short since small sound spell state still study table their
there these thing think three today under until value water where which
while white whole world would write answer around before better change
common create during enough follow global inside little member method
number object people person public record result return second should
simple single source string switch system through toward window without
another because between complete example general however history
material nothing present problem program provide question separate
anything available condition different important interface something
structure beautiful necessary knowledge everything everywhere
understand background particular throughout compatible
`.split(/\s+/).filter(Boolean);

const WORDS_BY_LENGTH = new Map();
for (const word of WORDS) {
  if (!WORDS_BY_LENGTH.has(word.length)) WORDS_BY_LENGTH.set(word.length, []);
  WORDS_BY_LENGTH.get(word.length).push(word);
}
const MAX_WORD = Math.max(...WORDS_BY_LENGTH.keys());
for (let length = 2; length <= MAX_WORD; length++) {
  if (!WORDS_BY_LENGTH.has(length)) throw new Error(`no word with ${length} letters`);
}
const LOWER = "abcdefghijklmnopqrstuvwxyz";
const HEX = "0123456789abcdef";

/// Returns random lowercase text of the given length made of words.
function words(length) {
  let out = "";
  let rest = length;
  while (rest > 0) {
    let next = rest <= MAX_WORD ? rest : 2 + Math.floor(random() * (MAX_WORD - 1));
    if (rest - next === 1) next -= 1;
    out += next === 1 ? pick(LOWER) : pick(WORDS_BY_LENGTH.get(next));
    rest -= next;
  }
  return out;
}

/// Replaces a run of letters with words in the same case.
function letters(run) {
  const replacement = words(run.length);
  if (run === run.toLowerCase()) return replacement;
  if (run.length > 1 && run === run.toUpperCase()) return replacement.toUpperCase();
  if (run[0] !== run[0].toLowerCase() && run.slice(1) === run.slice(1).toLowerCase()) {
    return replacement[0].toUpperCase() + replacement.slice(1);
  }
  // mixed case (identifiers, base64): keep the case of every letter
  let out = "";
  for (let i = 0; i < run.length; i++) {
    out += run[i] === run[i].toLowerCase() ? replacement[i] : replacement[i].toUpperCase();
  }
  return out;
}

const digits = (run) => Array.from(run, () => pick("0123456789")).join("");

// replacements for non-ASCII letters by their UTF-8 length
const UNICODE_LETTERS = {
  2: Array.from("äöüßéèêàáçñøåæœłžščřğışńóíúýþðαβγδελμπσωжзиклмнопрстуф"),
  3: Array.from("ḁḃḉḋḕḟḡḣḭḱḻṁṅṓṕṙṡṫṳṿẁẋẏẑあいうえおかきくけこさしすせそ中文字漢語日本한국어"),
  4: Array.from("𝒶𝒷𝒸𝒹𝑒𝒻𝑔𝒽𝒾𝒿𝓀𝓁𝓂𝓃𝑜𝓅𝓆𝓇𝓈𝓉𝓊𝓋𝓌𝓍𝓎𝓏"),
};

/// Scrubs free text.
function text(value) {
  return value.replace(/[A-Za-z]+|[0-9]+|\p{L}/gu, (match) => {
    if (/^[A-Za-z]/.test(match)) return letters(match);
    if (/^[0-9]/.test(match)) return digits(match);
    return pick(UNICODE_LETTERS[Buffer.byteLength(match)]);
  });
}

// ids: the same id always gets the same random replacement
const ids = new Map();
function id(value) {
  if (!ids.has(value)) {
    // keep the prefixes like `call_`, `toolu_` or `resp_`
    const replacement = value
      .split("|")
      .map((part) => {
        const [, prefix, rest] = part.match(/^([a-z]+_)?(.*)$/s);
        const hex = /^[0-9a-f-]+$/.test(rest);
        return (
          (prefix ?? "") +
          Array.from(rest, (c) => {
            if (hex) return c === "-" ? c : pick(HEX);
            if (/[0-9]/.test(c)) return pick("0123456789");
            if (/[a-z]/.test(c)) return pick(LOWER);
            if (/[A-Z]/.test(c)) return pick(LOWER).toUpperCase();
            return c;
          }).join("")
        );
      })
      .join("|");
    ids.set(value, replacement);
  }
  return ids.get(value);
}

/// Returns a PNG (base64) with about the given length that shows a pattern.
function png(length, index) {
  const bytes = Math.floor((length * 3) / 4);
  const width = 1024;
  const stride = 1 + width * 3;
  const height = Math.max(1, Math.round(bytes / stride));
  const raw = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const offset = y * stride + 1 + x * 3;
      raw[offset] = (x ^ y) & 0xff;
      raw[offset + 1] = (x + index * 40) & 0xff;
      raw[offset + 2] = (y * 2 + index * 70) & 0xff;
    }
  }
  const chunk = (type, data) => {
    const header = Buffer.alloc(8);
    header.writeUInt32BE(data.length, 0);
    header.write(type, 4, "latin1");
    const crc = Buffer.alloc(4);
    crc.writeUInt32BE(zlib.crc32(Buffer.concat([header.subarray(4), data])), 0);
    return Buffer.concat([header, data, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr.set([8, 2, 0, 0, 0], 8); // 8 bit RGB
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    // stored blocks: as large as the original (and cheap for git)
    chunk("IDAT", zlib.deflateSync(raw, { level: 0 })),
    chunk("IEND", Buffer.alloc(0)),
  ]).toString("base64");
}

// keys whose string values are kept (tags, providers, models, enums)
const KEEP = new Set([
  "type",
  "role",
  "api",
  "provider",
  "model",
  "modelId",
  "stopReason",
  "rawStopReason",
  "thinkingLevel",
  "providerThinkingLevel",
  "toolName",
  "kind",
  "phase",
  "configuredTransport",
  "fallbackTransport",
  "truncatedBy",
  "strict",
]);
const ID_KEYS = new Set([
  "id",
  "parentId",
  "targetId",
  "firstKeptEntryId",
  "toolCallId",
  "responseId",
  "searchId",
]);

const kept = new Map();
const keys = new Set();
let offset;
let images = 0;

function keep(path, value) {
  if (!kept.has(path)) kept.set(path, new Set());
  kept.get(path).add(value);
  return value;
}

function string(value, key, parent, path) {
  // the arguments of tool calls are free form
  if (path.includes(".arguments")) return text(value);
  if (key === "timestamp") return new Date(Date.parse(value) + offset).toISOString();
  if (ID_KEYS.has(key)) return id(value);
  if (parent?.type === "image" && key === "data") return png(value.length, images++);
  if (parent?.type === "image" && key === "mimeType") return "image/png";
  if (KEEP.has(key)) return keep(key, value);
  // tool names and the names of their parameters
  if (key === "name" && (parent?.type === "toolCall" || path.endsWith(".toolsAdded[]"))) {
    return keep("name", value);
  }
  if (key === "name" && path.endsWith(".diagnostics[].error")) return keep("error.name", value);
  if (key === "required" && path.includes(".parameters")) return keep("required", value);
  return text(value);
}

function walk(value, key, parent, path) {
  if (typeof value === "string") return string(value, key, parent, path);
  if (typeof value === "number" && key === "timestamp") return value + offset;
  if (Array.isArray(value)) return value.map((item) => walk(item, key, parent, path));
  if (value !== null && typeof value === "object") {
    const out = {};
    const inner = key === undefined ? "" : `${path}.${key}${Array.isArray(parent?.[key]) ? "[]" : ""}`;
    for (const [k, v] of Object.entries(value)) {
      keys.add(k);
      out[k] = walk(v, k, value, inner);
    }
    return out;
  }
  return value;
}

const lines = fs.readFileSync(input, "utf8").split("\n").filter(Boolean);
const header = JSON.parse(lines[0]);
if (header.type !== "session") throw new Error("not a session");
offset = Date.parse(start) - Date.parse(header.timestamp);

const out = lines.map((line) => JSON.stringify(walk(JSON.parse(line), undefined, undefined, "")));
fs.writeFileSync(output, out.join("\n") + "\n");

console.error(`${lines.length} entries, ${images} images`);
console.error("kept values:");
for (const [path, values] of kept) console.error(`  ${path}: ${[...values].join(", ")}`);
console.error(`keys: ${[...keys].sort().join(", ")}`);

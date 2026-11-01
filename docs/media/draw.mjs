// Draws the README's SVG figures (hero, feature icons), each in a light and a dark version.
// Run from the repository root:  node docs/media/draw.mjs
// No dependencies. The hero's flow is the launcher's real screen order: account tabs, the launch
// rows, enter, the project screen, then claude.
// Writes: hero-light.svg, hero-dark.svg, feature-accounts-light.svg, feature-accounts-dark.svg,
// feature-rows-light.svg, feature-rows-dark.svg, feature-projects-light.svg, feature-projects-dark.svg,
// feature-safe-light.svg, feature-safe-dark.svg, feature-sharing-light.svg, feature-sharing-dark.svg,
// feature-secrets-light.svg, feature-secrets-dark.svg, feature-hooks-light.svg, feature-hooks-dark.svg,
// feature-keys-light.svg, feature-keys-dark.svg.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';

const out = import.meta.dirname;
const SANS = 'system-ui, -apple-system, &quot;Segoe UI&quot;, Helvetica, Arial, sans-serif';
const MONO = 'ui-monospace, SFMono-Regular, &quot;Cascadia Mono&quot;, Consolas, Menlo, monospace';

// Accent = the launcher's own colours: the orange ✻ (xterm 209) and the green of a picked value.
const themes = {
  light: {
    bg: '#ffffff', border: '#d1d9e0', dot: '#e3e8ee', text: '#1f2328', muted: '#59636e',
    accent: '#c8532c', green: '#1a7f37', node: '#f6f8fa', nodeLine: '#d1d9e0', chip: '#f0f3f6',
    amber: '#9a6700', iconBg: '#fff1eb',
  },
  dark: {
    bg: '#10151c', border: '#2a313c', dot: '#1f262f', text: '#e6edf3', muted: '#9198a1',
    accent: '#ff875f', green: '#3fb950', node: '#161c24', nodeLine: '#343c47', chip: '#1b222b',
    amber: '#d29922', iconBg: '#2a1a14',
  },
};

const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

function card(w, h, p, body) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">
<defs><pattern id="dots" width="16" height="16" patternUnits="userSpaceOnUse"><circle cx="2" cy="2" r="1" fill="${p.dot}"/></pattern></defs>
<rect x="0.5" y="0.5" width="${w - 1}" height="${h - 1}" rx="16" fill="${p.bg}" stroke="${p.border}"/>
<rect x="1" y="1" width="${w - 2}" height="${h - 2}" rx="16" fill="url(#dots)"/>
${body}
</svg>
`;
}

function heroLeft(p) {
  const L = 72;
  return `
<text x="${L}" y="96" font-family="${SANS}" font-size="48" fill="${p.accent}">✻</text>
<text x="${L + 50}" y="88" font-family="${MONO}" font-size="18" fill="${p.muted}">a launcher for Claude Code · Windows</text>
<text x="${L - 4}" y="180" font-family="${SANS}" font-size="80" font-weight="700" fill="${p.text}" letter-spacing="-1.5">claude-auto</text>
<text x="${L}" y="230" font-family="${SANS}" font-size="30" font-weight="600" fill="${p.text}">Pick the account, model and project.</text>
<text x="${L}" y="270" font-family="${SANS}" font-size="21" fill="${p.muted}">One screen of remembered choices per account,</text>
<text x="${L}" y="300" font-family="${SANS}" font-size="21" fill="${p.muted}">then Claude Code starts. One native .exe.</text>
<rect x="${L}" y="326" width="236" height="44" rx="8" fill="${p.chip}" stroke="${p.nodeLine}"/>
<text x="${L + 18}" y="355" font-family="${MONO}" font-size="20" fill="${p.muted}">&gt; <tspan fill="${p.text}">claude-auto-rs</tspan><tspan fill="${p.accent}">▌</tspan></text>`;
}

// One box per screen, top to bottom; `edge` labels the keys that move to the next box.
function heroSteps(p) {
  return [
    { title: 'account', edge: 'w/s',
      detail: [['main ', p.green, true], ['34%', p.green], ['  ┊  team ', p.muted], ['12%', p.green], ['  ┊  night ', p.muted], ['78%', p.amber]] },
    { title: 'launch rows', edge: 'enter',
      detail: [['model ', p.muted], ['Opus', p.green, true], ['  effort ', p.muted], ['xhigh', p.green, true], ['  advisor ', p.muted], ['opus', p.green, true]] },
    { title: 'project', edge: 'enter · c · r · t',
      detail: [['new', p.green, true], ['  ·  continue  ·  resume  ·  worktree', p.muted]] },
    { title: 'claude',
      detail: [['claude ', p.text], ['--model opus[1m] --effort xhigh …', p.muted]] },
  ];
}

function heroFlow(p) {
  const x0 = 660, bw = 560, bh = 66, gap = 26, y0 = 30;
  const steps = heroSteps(p);
  return steps.map((s, i) => {
    const y = y0 + i * (bh + gap);
    const last = i === steps.length - 1;
    const spans = s.detail.map(([t, c, b]) => `<tspan fill="${c}"${b ? ' font-weight="700"' : ''}>${esc(t)}</tspan>`).join('');
    let g = `
<rect x="${x0}" y="${y}" width="${bw}" height="${bh}" rx="10" fill="${p.node}" stroke="${last ? p.accent : p.nodeLine}" stroke-width="${last ? 1.5 : 1}"/>
<circle cx="${x0 + 30}" cy="${y + bh / 2}" r="14" fill="${last ? p.accent : p.chip}" stroke="${last ? p.accent : p.nodeLine}"/>
<text x="${x0 + 30}" y="${y + bh / 2 + 5.5}" text-anchor="middle" font-family="${SANS}" font-size="16" font-weight="700" fill="${last ? p.bg : p.muted}">${i + 1}</text>
<text x="${x0 + 58}" y="${y + 28}" font-family="${SANS}" font-size="20" font-weight="700" fill="${p.text}">${s.title}</text>
<text x="${x0 + 58}" y="${y + 53}" font-family="${MONO}" font-size="18">${spans}</text>`;
    if (!last) {
      const ax = x0 + 30, ay = y + bh;
      g += `
<path d="M${ax} ${ay + 3} V${ay + gap - 5} M${ax - 4} ${ay + gap - 9} L${ax} ${ay + gap - 3} L${ax + 4} ${ay + gap - 9}" stroke="${p.muted}" stroke-width="1.5" fill="none"/>
<text x="${ax + 14}" y="${ay + gap / 2 + 6}" font-family="${MONO}" font-size="17" fill="${p.muted}">${s.edge}</text>`;
    }
    return g;
  }).join('');
}

const hero = (p) => card(1280, 400, p, heroLeft(p) + heroFlow(p));

// Feature icons: 96x96 rounded tiles, one stroke glyph each, shown at 48 px.
const glyphs = {
  accounts: (c) => `<rect x="22" y="30" width="16" height="10" rx="3" fill="${c}"/><rect x="40" y="30" width="16" height="10" rx="3" fill="none" stroke="${c}" stroke-width="3"/><rect x="58" y="30" width="16" height="10" rx="3" fill="none" stroke="${c}" stroke-width="3"/><rect x="22" y="44" width="52" height="24" rx="4" fill="none" stroke="${c}" stroke-width="3"/><path d="M28 58 H48" stroke="${c}" stroke-width="4" stroke-linecap="round"/>`,
  rows: (c) => `<path d="M24 32 H72 M24 48 H72 M24 64 H72" stroke="${c}" stroke-width="3" stroke-linecap="round"/><circle cx="56" cy="32" r="6" fill="${c}"/><circle cx="36" cy="48" r="6" fill="${c}"/><circle cx="62" cy="64" r="6" fill="${c}"/>`,
  projects: (c) => `<path d="M22 34 a4 4 0 0 1 4 -4 h14 l6 6 h24 a4 4 0 0 1 4 4 v24 a4 4 0 0 1 -4 4 H26 a4 4 0 0 1 -4 -4 Z" fill="none" stroke="${c}" stroke-width="3" stroke-linejoin="round"/><path d="M34 54 h28 M34 46 h18" stroke="${c}" stroke-width="3" stroke-linecap="round"/>`,
  safe: (c) => `<path d="M48 22 L70 30 V48 C70 62 60 70 48 75 C36 70 26 62 26 48 V30 Z" fill="none" stroke="${c}" stroke-width="3" stroke-linejoin="round"/><path d="M38 49 L45 56 L59 41" fill="none" stroke="${c}" stroke-width="3.5" stroke-linecap="round" stroke-linejoin="round"/>`,
  sharing: (c) => `<circle cx="30" cy="48" r="8" fill="none" stroke="${c}" stroke-width="3"/><circle cx="66" cy="30" r="8" fill="none" stroke="${c}" stroke-width="3"/><circle cx="66" cy="66" r="8" fill="none" stroke="${c}" stroke-width="3"/><path d="M37 44 L59 34 M37 52 L59 62" stroke="${c}" stroke-width="3"/>`,
  secrets: (c) => `<circle cx="36" cy="48" r="12" fill="none" stroke="${c}" stroke-width="3"/><circle cx="36" cy="48" r="3.5" fill="${c}"/><path d="M48 48 H74 M66 48 V58 M74 48 V56" stroke="${c}" stroke-width="3" stroke-linecap="round"/>`,
  hooks: (c) => `<path d="M52 20 L32 52 H48 L44 76 L66 42 H50 Z" fill="none" stroke="${c}" stroke-width="3" stroke-linejoin="round"/>`,
  keys: (c) => `<rect x="18" y="32" width="60" height="34" rx="5" fill="none" stroke="${c}" stroke-width="3"/><path d="M28 42 h2 M38 42 h2 M48 42 h2 M58 42 h2 M66 42 h2 M28 50 h2 M38 50 h2 M48 50 h2 M58 50 h2 M66 50 h2 M34 58 H62" stroke="${c}" stroke-width="3.5" stroke-linecap="round"/>`,
};

const icon = (p, g) => `<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96" viewBox="0 0 96 96">
<rect x="1" y="1" width="94" height="94" rx="22" fill="${p.iconBg}" stroke="${p.accent}" stroke-opacity="0.35" stroke-width="2"/>
${glyphs[g](p.accent)}
</svg>
`;

let n = 0;
for (const [name, p] of Object.entries(themes)) {
  writeFileSync(join(out, `hero-${name}.svg`), hero(p)); n++;
  for (const g of Object.keys(glyphs)) { writeFileSync(join(out, `feature-${g}-${name}.svg`), icon(p, g)); n++; }
}
console.log(`draw.mjs: ${n} files written to ${out}`);

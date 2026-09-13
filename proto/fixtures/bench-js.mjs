// Référence JS sur CETTE machine, avec le moteur actuel, sur le fichier fixe.
// Trois mesures : décodage complet, //replace sur toute la région, empreinte.
import { readFileSync } from 'node:fs';
import { RegionStore } from '../../../ExeWorldEdit/packages/we-engine/src/worldedit/regionStore.js';

const MCA = './r.0.0.mca';
const BBOX = { min: { x: 0, y: -64, z: 0 }, max: { x: 511, y: 127, z: 511 } };
const mb = (n) => (n / 1048576).toFixed(0);

const STONE = { Name: 'minecraft:stone', Properties: null };
const DIRT = { Name: 'minecraft:dirt', Properties: null };
const buffer = readFileSync(MCA);

// ── 1. décodage complet ────────────────────────────────────────────────
let t = performance.now();
const store = new RegionStore([{ regionX: 0, regionZ: 0, buffer }]);
await store.warmup(BBOX);
const decodeMs = performance.now() - t;
const afterDecode = process.memoryUsage();

let sections = 0;
for (const r of store.regions.values()) for (const rec of r.chunks.values()) if (rec.sections) sections += rec.sections.size;

console.log(`décodage région pleine   ${decodeMs.toFixed(0)} ms   ${sections} sections   heap ${mb(afterDecode.heapUsed)} Mo   rss ${mb(afterDecode.rss)} Mo`);

// ── 2. //replace stone → dirt sur toute la région ───────────────────────
// Chemin réel du moteur : lecture + écriture bloc par bloc.
t = performance.now();
let changed = 0;
for (let y = -64; y <= 127; y++) {
  for (let z = 0; z < 512; z++) {
    for (let x = 0; x < 512; x++) {
      if (store.matchesAt(x, y, z, STONE)) {
        store.setBlock(x, y, z, DIRT);
        changed++;
      }
    }
  }
}
const replaceMs = performance.now() - t;
const afterReplace = process.memoryUsage();
const cells = 512 * 512 * 192;
console.log(`//replace stone→dirt     ${replaceMs.toFixed(0)} ms   ${changed.toLocaleString('fr')} blocs changés sur ${cells.toLocaleString('fr')} cases`);
console.log(`                         ${(cells / replaceMs / 1000).toFixed(1)} M cases/s   rss ${mb(afterReplace.rss)} Mo`);

// ── 3. empreinte par bloc ──────────────────────────────────────────────
console.log(`empreinte décodée        ${(afterDecode.rss / (sections * 4096)).toFixed(2)} o/bloc résident`);

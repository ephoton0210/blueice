// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Publish actual observations only after every identified defect has a CLI replay.
const fs = require('node:fs'), path = require('node:path'), assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const { sha256 } = require('./ge5_inputs.cjs');
const root = path.resolve(__dirname, '../../../..'), phase = path.dirname(__dirname);
function argument(name) {
  const index = process.argv.indexOf(name); assert(index > 0 && process.argv[index + 1], name);
  return process.argv[index + 1];
}
const fixture = path.join(root, 'backend/bluets/tests/fixtures/ge5_source_maps/native-map-defects.json');
const ledgerBytes = fs.readFileSync(fixture), ledger = JSON.parse(ledgerBytes);
const blueBytes = fs.readFileSync(argument('--blue-report')), blue = JSON.parse(blueBytes);
const nativeLog = fs.readFileSync(argument('--native-log'), 'utf8');
const native = JSON.parse(nativeLog.trim().split('\n').at(-1));
const blueLog = fs.readFileSync(argument('--blue-log'), 'utf8');
const nodeLog = fs.readFileSync(argument('--node-log'), 'utf8');
const clippyLog = fs.readFileSync(argument('--clippy-log'), 'utf8');
assert.equal(native.scope, 'all-8407-configurations'); assert.equal(native.configurations, 8407);
assert.equal(native.maps, 20590); assert.equal(native.changedMaps, 572);
for (const key of ['strictFailures', 'statementMappingDifferences', 'nonMapArtifactDifferences', 'repeatedBuildDifferences']) assert.equal(native[key], 0);
assert.equal(native.ledgerSha256, sha256(ledgerBytes)); assert.equal(blue.ledgerSha256, native.ledgerSha256);
assert.equal(blue.configurations, 452); assert.equal(blue.maps, 572); assert.equal(blue.repeatBuilds, 2);
assert(blueLog.includes('test result: ok. 2 passed; 0 failed; 0 ignored;'));
assert(nodeLog.includes('tests 9')); assert(nodeLog.includes('pass 9')); assert(nodeLog.includes('fail 0'));
assert(clippyLog.includes('Finished')); assert(!clippyLog.includes('error:'));
const formatting = spawnSync('cargo', ['fmt', '--all', '--', '--check'], { cwd: root, encoding: 'utf8' });
assert.equal(formatting.status, 0, formatting.stdout + formatting.stderr);
const observed = new Map(blue.verified.map(row => [row.id, row])); assert.equal(observed.size, 572);
for (const row of ledger.defects) {
  const result = observed.get(row.id); assert(result);
  assert.equal(result.caseId, row.caseId); assert.equal(result.map, row.map);
  assert.equal(result.strictBounds, true); assert.equal(result.generatedExecuteBracesUnmapped, true);
}
assert.equal(sha256(fs.readFileSync(path.join(root, 'target/debug/bluetsc'))), blue.bluetsBinarySha256);
const files = {};
function sources(directory) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const name = path.join(directory, entry.name);
    if (entry.isDirectory()) sources(name);
    else if (name.endsWith('.rs')) files[path.relative(root, name)] = sha256(fs.readFileSync(name));
  }
}
sources(path.join(root, 'backend/bluets/src'));
for (const name of ['ge5_native_patch.cjs', 'ge5_native_compile.cjs', 'ge5_native_patch.test.cjs',
  'verify_ge5_native_patch.cjs', 'report_ge5_native_defects.cjs', 'ge5_maps.cjs']) {
  const file = path.join(__dirname, name); files[path.relative(root, file)] = sha256(fs.readFileSync(file));
}
const measurement = { schema: 1, scope: 'G-E5 Native System EOF+1 defect correction; G-E5 remains open',
  upstreamStatus: ledger.upstreamStatus, ledgerSha256: sha256(ledgerBytes),
  stockBundleSha256: ledger.stockBundleSha256, patchedBundleSha256: ledger.patchedBundleSha256,
  native, bluets: blue, checks: { nodeTests: 9, rustTests: 2, formatExitStatus: formatting.status,
    clippy: 'blueice-bluets and blueice-bluets-bluejs; all targets; warnings denied' },
  logs: { nativeSha256: sha256(nativeLog), bluetsSha256: sha256(blueLog), nodeSha256: sha256(nodeLog), clippySha256: sha256(clippyLog) },
  sourceFiles: Object.fromEntries(Object.entries(files).sort(([a], [b]) => a.localeCompare(b))) };
const counts = new Map();
for (const row of ledger.defects) counts.set(row.form, (counts.get(row.form) || 0) + 1);
const lines = [
  '# G-E5 Native source-map defect ledger', '',
  '已逐筆重現並驗證 **572 / 572** 筆來源座標越界映射，涵蓋 **452 個配置、138 份不同 raw map、14 個來源家族**。',
  '它們都來自 TypeScript 5.9.3 的 System `execute` 生成區塊，具有同一根因，不能計為 572 個獨立根因。', '',
  '狀態：**本機獨立修補版已驗證；未提交上游、未取得上游合入**。原版套件及凍結的 `cases.json` / `reference.json` 均保留原始 bytes。',
  '本清單屬於既有 G-E5，沒有新增 TODO 項目，也不表示 G-E5 的其他驗收已完成。', '',
  '## 根因與修正', '',
  'System 轉換器用來源檔 statements 建立生成的 `execute` function block，其 statements.end 落在來源 EOF。',
  'printer 在輸出結尾 `}` 時先記錄 EOF，再把 token 位置加一，產生空 EOF 行 column 1 的越界來源座標。',
  '修補在這個生成 block 設定 `NoTokenSourceMaps`，因此生成括號不宣稱來源位置，區塊內實際敘述的所有映射則保留。',
  '沒有事後改寫 map、截斷座標、忽略失敗配置或重錄原始 Native 基準。', '',
  '上游實作：[TypeScript v5.9.3 emitter.ts](https://github.com/microsoft/TypeScript/blob/v5.9.3/src/compiler/emitter.ts)。',
  '精確修補位置是同版 bundle 的 `createSystemModuleBody`；bundle SHA-256 與替換內容均由 [patch loader](tools/ge5_native_patch.cjs) 固定。',
  '這是來源文字邊界與生成程式 provenance 的缺陷，不將它擴大宣稱為 ECMA-426 編碼語法錯誤。', '',
  '## 實際驗證', '',
  '- 原版與修補版重跑全部 **516 個 System 配置**，各兩次；所有 572 筆原版缺陷均能重現。',
  '- 修補版重跑全部 **8,407 個配置、20,590 份 map**，每個配置兩次；只改變這 572 份 map。',
  '- 每份受影響 map 僅移除生成結尾 `}` 的 EOF / EOF+1 兩個 segments；其餘完整有序 segments 與 metadata 不變。',
  '- 所有 JavaScript、宣告檔、宣告 map、診斷、選入來源與產物清單不變；原有同座標有序 Native segments 也保留。',
  '- BlueTSC 以公開 CLI 重跑 **452 個配置、572 份對應 map**，各兩次：完整來源邊界、宣告 bytes、產物清單通過；生成 execute 的開、閉括號沒有來源座標。',
  '- 保留原版壞 map 作負控制：BlueTS 的公開 lookup 會拒絕整份 map，即使查詢位置在缺陷之前。',
  '- 9 項 Node 回歸及 2 項 Rust 回歸通過；含 fresh tsc 程序、無尾端換行、CRLF、astral、型別空模組、hoisted function、其他 module 控制及套件 hash 變更拒絕。', '',
  'Rust 格式檢查及兩個 BlueTS crate 的 all-target Clippy（warnings denied）通過。', '',
  '[完整量測及每筆 BlueTSC 結果](GE5_NATIVE_MAP_MEASUREMENT.json) · [機器可讀原始／修補證據](../../../backend/bluets/tests/fixtures/ge5_source_maps/native-map-defects.json)', '',
  '## 來源家族分布', '', '| 家族 | 映射筆數 |', '| --- | ---: |',
  ...[...counts].map(([name, count]) => '| `' + name + '` | ' + count + ' |'),
  '| **合計** | **572** |', '',
  '## 572 筆完整清單', '',
  '座標皆從零起算。每列的修補版 Native 與 BlueTSC 均已實測通過；完整 SHA-256、來源 bytes 身分、刪除 segments 與各次重跑條件見上方 JSON。', '',
  '| ID | 配置 | Map | 原版生成位置 | 原版越界來源位置 | 本機 Native / BlueTSC |',
  '| --- | --- | --- | --- | --- | --- |',
  ...ledger.defects.map(row => {
    const [line, column, , originalLine, originalColumn] = row.issue.segment;
    return '| ' + [row.id, '`' + row.caseId + '`', '`' + row.map + '`', line + ':' + column,
      '`' + row.source + '` ' + originalLine + ':' + originalColumn, '通過 / 通過'].join(' | ') + ' |';
  }), '', '## 重跑', '', '先將 `BLUEICE_BLUETSC_ORACLE` 設為原版 TypeScript 5.9.3 的 `bin/tsc`，然後執行：', '',
  '```sh',
  'node --test development/browser_core/phase-18-bluets/tools/ge5_maps.test.cjs development/browser_core/phase-18-bluets/tools/ge5_native_patch.test.cjs',
  'node development/browser_core/phase-18-bluets/tools/verify_ge5_native_patch.cjs --all',
  'CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p blueice-bluets --test ge5_source_maps native_defects -- --nocapture',
  '```', '',
  '一般重跑只核對 ledger，不改寫它。`--write` 僅供明確產生獨立修補證據；stock reference 沒有寫入路徑。', '',
];
const documents = { 'GE5_NATIVE_MAP_MEASUREMENT.json': JSON.stringify(measurement, null, 2) + '\n',
  'GE5_NATIVE_MAP_DEFECTS.md': lines.join('\n') };
for (const [name, text] of Object.entries(documents)) {
  const file = path.join(phase, name);
  if (process.argv.includes('--write')) fs.writeFileSync(file, text);
  else assert.equal(fs.readFileSync(file, 'utf8'), text, name + ': evidence changed');
}
console.log(JSON.stringify({ defects: observed.size, configurations: blue.configurations,
  nativeConfigurations: native.configurations, ledgerSha256: native.ledgerSha256 }));

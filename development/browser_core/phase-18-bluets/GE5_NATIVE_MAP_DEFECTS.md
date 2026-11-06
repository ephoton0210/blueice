# G-E5 Native source-map defect ledger

已逐筆重現並驗證 **572 / 572** 筆來源座標越界映射，涵蓋 **452 個配置、138 份不同 raw map、14 個來源家族**。
它們都來自 TypeScript 5.9.3 的 System `execute` 生成區塊，具有同一根因，不能計為 572 個獨立根因。

狀態：**本機獨立修補版已驗證；未提交上游、未取得上游合入**。原版套件及凍結的 `cases.json` / `reference.json` 均保留原始 bytes。
本清單屬於既有 G-E5，沒有新增 TODO 項目，也不表示 G-E5 的其他驗收已完成。

## 根因與修正

System 轉換器用來源檔 statements 建立生成的 `execute` function block，其 statements.end 落在來源 EOF。
printer 在輸出結尾 `}` 時先記錄 EOF，再把 token 位置加一，產生空 EOF 行 column 1 的越界來源座標。
修補在這個生成 block 設定 `NoTokenSourceMaps`，因此生成括號不宣稱來源位置，區塊內實際敘述的所有映射則保留。
沒有事後改寫 map、截斷座標、忽略失敗配置或重錄原始 Native 基準。

上游實作：[TypeScript v5.9.3 emitter.ts](https://github.com/microsoft/TypeScript/blob/v5.9.3/src/compiler/emitter.ts)。
精確修補位置是同版 bundle 的 `createSystemModuleBody`；bundle SHA-256 與替換內容均由 [patch loader](tools/ge5_native_patch.cjs) 固定。
這是來源文字邊界與生成程式 provenance 的缺陷，不將它擴大宣稱為 ECMA-426 編碼語法錯誤。

## 實際驗證

- 原版與修補版重跑全部 **516 個 System 配置**，各兩次；所有 572 筆原版缺陷均能重現。
- 修補版重跑全部 **8,407 個配置、20,590 份 map**，每個配置兩次；只改變這 572 份 map。
- 每份受影響 map 僅移除生成結尾 `}` 的 EOF / EOF+1 兩個 segments；其餘完整有序 segments 與 metadata 不變。
- 所有 JavaScript、宣告檔、宣告 map、診斷、選入來源與產物清單不變；原有同座標有序 Native segments 也保留。
- BlueTSC 以公開 CLI 重跑 **452 個配置、572 份對應 map**，各兩次：完整來源邊界、宣告 bytes、產物清單通過；生成 execute 的開、閉括號沒有來源座標。
- 保留原版壞 map 作負控制：BlueTS 的公開 lookup 會拒絕整份 map，即使查詢位置在缺陷之前。
- 9 項 Node 回歸及 2 項 Rust 回歸通過；含 fresh tsc 程序、無尾端換行、CRLF、astral、型別空模組、hoisted function、其他 module 控制及套件 hash 變更拒絕。

Rust 格式檢查及兩個 BlueTS crate 的 all-target Clippy（warnings denied）通過。

[完整量測及每筆 BlueTSC 結果](GE5_NATIVE_MAP_MEASUREMENT.json) · [機器可讀原始／修補證據](../../../backend/bluets/tests/fixtures/ge5_source_maps/native-map-defects.json)

## 來源家族分布

| 家族 | 映射筆數 |
| --- | ---: |
| `ge2_module_helpers/inherit-constructor-spread` | 40 |
| `ge2_module_helpers/async-arrow-finally` | 40 |
| `ge2_module_helpers/generator-switch-delegation` | 40 |
| `ge2_module_helpers/async-generator-finally` | 40 |
| `ge2_module_helpers/object-rest-spread` | 28 |
| `ge2_module_helpers/iterable-bindings-forof` | 8 |
| `ge2_module_helpers/sparse-array-call-spread` | 40 |
| `ge2_module_helpers/tagged-template` | 40 |
| `ge2_module_helpers/private-accessor` | 8 |
| `ge2_module_helpers/standard-decorator` | 40 |
| `ge2_module_helpers/legacy-parameter-decorator` | 40 |
| `ge2_module_helpers/interop-exportstar-cycle` | 160 |
| `ge2_module_helpers/optional-nullish` | 8 |
| `ge2_module_helpers/helper-name-collision` | 40 |
| **合計** | **572** |

## 572 筆完整清單

座標皆從零起算。每列的修補版 Native 與 BlueTSC 均已實測通過；完整 SHA-256、來源 bytes 身分、刪除 segments 與各次重跑條件見上方 JSON。

| ID | 配置 | Map | 原版生成位置 | 原版越界來源位置 | 本機 Native / BlueTSC |
| --- | --- | --- | --- | --- | --- |
| NATIVE-MAP-0001 | `ge2_module_helpers/inherit-constructor-spread-system-es5-i0-n0/original/external` | `out/main.js.map` | 39:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0002 | `ge2_module_helpers/inherit-constructor-spread-system-es5-i0-n0/crlfAstral/external` | `out/main.js.map` | 39:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0003 | `ge2_module_helpers/inherit-constructor-spread-system-es5-i0-n1/original/external` | `out/main.js.map` | 24:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0004 | `ge2_module_helpers/inherit-constructor-spread-system-es5-i0-n1/crlfAstral/external` | `out/main.js.map` | 24:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0005 | `ge2_module_helpers/inherit-constructor-spread-system-es5-i1-n0/original/external` | `out/main.js.map` | 28:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0006 | `ge2_module_helpers/inherit-constructor-spread-system-es5-i1-n0/crlfAstral/external` | `out/main.js.map` | 28:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0007 | `ge2_module_helpers/inherit-constructor-spread-system-es5-i1-n1/original/external` | `out/main.js.map` | 28:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0008 | `ge2_module_helpers/inherit-constructor-spread-system-es5-i1-n1/crlfAstral/external` | `out/main.js.map` | 28:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0009 | `ge2_module_helpers/inherit-constructor-spread-system-es2015-i0-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0010 | `ge2_module_helpers/inherit-constructor-spread-system-es2015-i0-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0011 | `ge2_module_helpers/inherit-constructor-spread-system-es2015-i0-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0012 | `ge2_module_helpers/inherit-constructor-spread-system-es2015-i0-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0013 | `ge2_module_helpers/inherit-constructor-spread-system-es2015-i1-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0014 | `ge2_module_helpers/inherit-constructor-spread-system-es2015-i1-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0015 | `ge2_module_helpers/inherit-constructor-spread-system-es2015-i1-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0016 | `ge2_module_helpers/inherit-constructor-spread-system-es2015-i1-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0017 | `ge2_module_helpers/inherit-constructor-spread-system-es2017-i0-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0018 | `ge2_module_helpers/inherit-constructor-spread-system-es2017-i0-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0019 | `ge2_module_helpers/inherit-constructor-spread-system-es2017-i0-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0020 | `ge2_module_helpers/inherit-constructor-spread-system-es2017-i0-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0021 | `ge2_module_helpers/inherit-constructor-spread-system-es2017-i1-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0022 | `ge2_module_helpers/inherit-constructor-spread-system-es2017-i1-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0023 | `ge2_module_helpers/inherit-constructor-spread-system-es2017-i1-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0024 | `ge2_module_helpers/inherit-constructor-spread-system-es2017-i1-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0025 | `ge2_module_helpers/inherit-constructor-spread-system-es2018-i0-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0026 | `ge2_module_helpers/inherit-constructor-spread-system-es2018-i0-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0027 | `ge2_module_helpers/inherit-constructor-spread-system-es2018-i0-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0028 | `ge2_module_helpers/inherit-constructor-spread-system-es2018-i0-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0029 | `ge2_module_helpers/inherit-constructor-spread-system-es2018-i1-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0030 | `ge2_module_helpers/inherit-constructor-spread-system-es2018-i1-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0031 | `ge2_module_helpers/inherit-constructor-spread-system-es2018-i1-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0032 | `ge2_module_helpers/inherit-constructor-spread-system-es2018-i1-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0033 | `ge2_module_helpers/inherit-constructor-spread-system-es2022-i0-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0034 | `ge2_module_helpers/inherit-constructor-spread-system-es2022-i0-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0035 | `ge2_module_helpers/inherit-constructor-spread-system-es2022-i0-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0036 | `ge2_module_helpers/inherit-constructor-spread-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0037 | `ge2_module_helpers/inherit-constructor-spread-system-es2022-i1-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0038 | `ge2_module_helpers/inherit-constructor-spread-system-es2022-i1-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0039 | `ge2_module_helpers/inherit-constructor-spread-system-es2022-i1-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0040 | `ge2_module_helpers/inherit-constructor-spread-system-es2022-i1-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0041 | `ge2_module_helpers/async-arrow-finally-system-es5-i0-n0/original/external` | `out/main.js.map` | 74:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0042 | `ge2_module_helpers/async-arrow-finally-system-es5-i0-n0/crlfAstral/external` | `out/main.js.map` | 74:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0043 | `ge2_module_helpers/async-arrow-finally-system-es5-i0-n1/original/external` | `out/main.js.map` | 38:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0044 | `ge2_module_helpers/async-arrow-finally-system-es5-i0-n1/crlfAstral/external` | `out/main.js.map` | 38:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0045 | `ge2_module_helpers/async-arrow-finally-system-es5-i1-n0/original/external` | `out/main.js.map` | 42:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0046 | `ge2_module_helpers/async-arrow-finally-system-es5-i1-n0/crlfAstral/external` | `out/main.js.map` | 42:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0047 | `ge2_module_helpers/async-arrow-finally-system-es5-i1-n1/original/external` | `out/main.js.map` | 42:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0048 | `ge2_module_helpers/async-arrow-finally-system-es5-i1-n1/crlfAstral/external` | `out/main.js.map` | 42:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0049 | `ge2_module_helpers/async-arrow-finally-system-es2015-i0-n0/original/external` | `out/main.js.map` | 28:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0050 | `ge2_module_helpers/async-arrow-finally-system-es2015-i0-n0/crlfAstral/external` | `out/main.js.map` | 28:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0051 | `ge2_module_helpers/async-arrow-finally-system-es2015-i0-n1/original/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0052 | `ge2_module_helpers/async-arrow-finally-system-es2015-i0-n1/crlfAstral/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0053 | `ge2_module_helpers/async-arrow-finally-system-es2015-i1-n0/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0054 | `ge2_module_helpers/async-arrow-finally-system-es2015-i1-n0/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0055 | `ge2_module_helpers/async-arrow-finally-system-es2015-i1-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0056 | `ge2_module_helpers/async-arrow-finally-system-es2015-i1-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0057 | `ge2_module_helpers/async-arrow-finally-system-es2017-i0-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0058 | `ge2_module_helpers/async-arrow-finally-system-es2017-i0-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0059 | `ge2_module_helpers/async-arrow-finally-system-es2017-i0-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0060 | `ge2_module_helpers/async-arrow-finally-system-es2017-i0-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0061 | `ge2_module_helpers/async-arrow-finally-system-es2017-i1-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0062 | `ge2_module_helpers/async-arrow-finally-system-es2017-i1-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0063 | `ge2_module_helpers/async-arrow-finally-system-es2017-i1-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0064 | `ge2_module_helpers/async-arrow-finally-system-es2017-i1-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0065 | `ge2_module_helpers/async-arrow-finally-system-es2018-i0-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0066 | `ge2_module_helpers/async-arrow-finally-system-es2018-i0-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0067 | `ge2_module_helpers/async-arrow-finally-system-es2018-i0-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0068 | `ge2_module_helpers/async-arrow-finally-system-es2018-i0-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0069 | `ge2_module_helpers/async-arrow-finally-system-es2018-i1-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0070 | `ge2_module_helpers/async-arrow-finally-system-es2018-i1-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0071 | `ge2_module_helpers/async-arrow-finally-system-es2018-i1-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0072 | `ge2_module_helpers/async-arrow-finally-system-es2018-i1-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0073 | `ge2_module_helpers/async-arrow-finally-system-es2022-i0-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0074 | `ge2_module_helpers/async-arrow-finally-system-es2022-i0-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0075 | `ge2_module_helpers/async-arrow-finally-system-es2022-i0-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0076 | `ge2_module_helpers/async-arrow-finally-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0077 | `ge2_module_helpers/async-arrow-finally-system-es2022-i1-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0078 | `ge2_module_helpers/async-arrow-finally-system-es2022-i1-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0079 | `ge2_module_helpers/async-arrow-finally-system-es2022-i1-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0080 | `ge2_module_helpers/async-arrow-finally-system-es2022-i1-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0081 | `ge2_module_helpers/generator-switch-delegation-system-es5-i0-n0/original/external` | `out/main.js.map` | 77:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0082 | `ge2_module_helpers/generator-switch-delegation-system-es5-i0-n0/crlfAstral/external` | `out/main.js.map` | 77:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0083 | `ge2_module_helpers/generator-switch-delegation-system-es5-i0-n1/original/external` | `out/main.js.map` | 39:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0084 | `ge2_module_helpers/generator-switch-delegation-system-es5-i0-n1/crlfAstral/external` | `out/main.js.map` | 39:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0085 | `ge2_module_helpers/generator-switch-delegation-system-es5-i1-n0/original/external` | `out/main.js.map` | 43:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0086 | `ge2_module_helpers/generator-switch-delegation-system-es5-i1-n0/crlfAstral/external` | `out/main.js.map` | 43:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0087 | `ge2_module_helpers/generator-switch-delegation-system-es5-i1-n1/original/external` | `out/main.js.map` | 43:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0088 | `ge2_module_helpers/generator-switch-delegation-system-es5-i1-n1/crlfAstral/external` | `out/main.js.map` | 43:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0089 | `ge2_module_helpers/generator-switch-delegation-system-es2015-i0-n0/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0090 | `ge2_module_helpers/generator-switch-delegation-system-es2015-i0-n0/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0091 | `ge2_module_helpers/generator-switch-delegation-system-es2015-i0-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0092 | `ge2_module_helpers/generator-switch-delegation-system-es2015-i0-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0093 | `ge2_module_helpers/generator-switch-delegation-system-es2015-i1-n0/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0094 | `ge2_module_helpers/generator-switch-delegation-system-es2015-i1-n0/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0095 | `ge2_module_helpers/generator-switch-delegation-system-es2015-i1-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0096 | `ge2_module_helpers/generator-switch-delegation-system-es2015-i1-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0097 | `ge2_module_helpers/generator-switch-delegation-system-es2017-i0-n0/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0098 | `ge2_module_helpers/generator-switch-delegation-system-es2017-i0-n0/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0099 | `ge2_module_helpers/generator-switch-delegation-system-es2017-i0-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0100 | `ge2_module_helpers/generator-switch-delegation-system-es2017-i0-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0101 | `ge2_module_helpers/generator-switch-delegation-system-es2017-i1-n0/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0102 | `ge2_module_helpers/generator-switch-delegation-system-es2017-i1-n0/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0103 | `ge2_module_helpers/generator-switch-delegation-system-es2017-i1-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0104 | `ge2_module_helpers/generator-switch-delegation-system-es2017-i1-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0105 | `ge2_module_helpers/generator-switch-delegation-system-es2018-i0-n0/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0106 | `ge2_module_helpers/generator-switch-delegation-system-es2018-i0-n0/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0107 | `ge2_module_helpers/generator-switch-delegation-system-es2018-i0-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0108 | `ge2_module_helpers/generator-switch-delegation-system-es2018-i0-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0109 | `ge2_module_helpers/generator-switch-delegation-system-es2018-i1-n0/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0110 | `ge2_module_helpers/generator-switch-delegation-system-es2018-i1-n0/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0111 | `ge2_module_helpers/generator-switch-delegation-system-es2018-i1-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0112 | `ge2_module_helpers/generator-switch-delegation-system-es2018-i1-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0113 | `ge2_module_helpers/generator-switch-delegation-system-es2022-i0-n0/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0114 | `ge2_module_helpers/generator-switch-delegation-system-es2022-i0-n0/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0115 | `ge2_module_helpers/generator-switch-delegation-system-es2022-i0-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0116 | `ge2_module_helpers/generator-switch-delegation-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0117 | `ge2_module_helpers/generator-switch-delegation-system-es2022-i1-n0/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0118 | `ge2_module_helpers/generator-switch-delegation-system-es2022-i1-n0/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0119 | `ge2_module_helpers/generator-switch-delegation-system-es2022-i1-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0120 | `ge2_module_helpers/generator-switch-delegation-system-es2022-i1-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0121 | `ge2_module_helpers/async-generator-finally-system-es5-i0-n0/original/external` | `out/main.js.map` | 98:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0122 | `ge2_module_helpers/async-generator-finally-system-es5-i0-n0/crlfAstral/external` | `out/main.js.map` | 98:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0123 | `ge2_module_helpers/async-generator-finally-system-es5-i0-n1/original/external` | `out/main.js.map` | 49:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0124 | `ge2_module_helpers/async-generator-finally-system-es5-i0-n1/crlfAstral/external` | `out/main.js.map` | 49:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0125 | `ge2_module_helpers/async-generator-finally-system-es5-i1-n0/original/external` | `out/main.js.map` | 53:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0126 | `ge2_module_helpers/async-generator-finally-system-es5-i1-n0/crlfAstral/external` | `out/main.js.map` | 53:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0127 | `ge2_module_helpers/async-generator-finally-system-es5-i1-n1/original/external` | `out/main.js.map` | 53:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0128 | `ge2_module_helpers/async-generator-finally-system-es5-i1-n1/crlfAstral/external` | `out/main.js.map` | 53:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0129 | `ge2_module_helpers/async-generator-finally-system-es2015-i0-n0/original/external` | `out/main.js.map` | 43:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0130 | `ge2_module_helpers/async-generator-finally-system-es2015-i0-n0/crlfAstral/external` | `out/main.js.map` | 43:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0131 | `ge2_module_helpers/async-generator-finally-system-es2015-i0-n1/original/external` | `out/main.js.map` | 21:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0132 | `ge2_module_helpers/async-generator-finally-system-es2015-i0-n1/crlfAstral/external` | `out/main.js.map` | 21:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0133 | `ge2_module_helpers/async-generator-finally-system-es2015-i1-n0/original/external` | `out/main.js.map` | 25:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0134 | `ge2_module_helpers/async-generator-finally-system-es2015-i1-n0/crlfAstral/external` | `out/main.js.map` | 25:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0135 | `ge2_module_helpers/async-generator-finally-system-es2015-i1-n1/original/external` | `out/main.js.map` | 25:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0136 | `ge2_module_helpers/async-generator-finally-system-es2015-i1-n1/crlfAstral/external` | `out/main.js.map` | 25:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0137 | `ge2_module_helpers/async-generator-finally-system-es2017-i0-n0/original/external` | `out/main.js.map` | 32:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0138 | `ge2_module_helpers/async-generator-finally-system-es2017-i0-n0/crlfAstral/external` | `out/main.js.map` | 32:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0139 | `ge2_module_helpers/async-generator-finally-system-es2017-i0-n1/original/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0140 | `ge2_module_helpers/async-generator-finally-system-es2017-i0-n1/crlfAstral/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0141 | `ge2_module_helpers/async-generator-finally-system-es2017-i1-n0/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0142 | `ge2_module_helpers/async-generator-finally-system-es2017-i1-n0/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0143 | `ge2_module_helpers/async-generator-finally-system-es2017-i1-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0144 | `ge2_module_helpers/async-generator-finally-system-es2017-i1-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0145 | `ge2_module_helpers/async-generator-finally-system-es2018-i0-n0/original/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0146 | `ge2_module_helpers/async-generator-finally-system-es2018-i0-n0/crlfAstral/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0147 | `ge2_module_helpers/async-generator-finally-system-es2018-i0-n1/original/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0148 | `ge2_module_helpers/async-generator-finally-system-es2018-i0-n1/crlfAstral/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0149 | `ge2_module_helpers/async-generator-finally-system-es2018-i1-n0/original/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0150 | `ge2_module_helpers/async-generator-finally-system-es2018-i1-n0/crlfAstral/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0151 | `ge2_module_helpers/async-generator-finally-system-es2018-i1-n1/original/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0152 | `ge2_module_helpers/async-generator-finally-system-es2018-i1-n1/crlfAstral/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0153 | `ge2_module_helpers/async-generator-finally-system-es2022-i0-n0/original/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0154 | `ge2_module_helpers/async-generator-finally-system-es2022-i0-n0/crlfAstral/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0155 | `ge2_module_helpers/async-generator-finally-system-es2022-i0-n1/original/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0156 | `ge2_module_helpers/async-generator-finally-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0157 | `ge2_module_helpers/async-generator-finally-system-es2022-i1-n0/original/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0158 | `ge2_module_helpers/async-generator-finally-system-es2022-i1-n0/crlfAstral/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0159 | `ge2_module_helpers/async-generator-finally-system-es2022-i1-n1/original/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0160 | `ge2_module_helpers/async-generator-finally-system-es2022-i1-n1/crlfAstral/external` | `out/main.js.map` | 19:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0161 | `ge2_module_helpers/object-rest-spread-system-es5-i0-n0/original/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0162 | `ge2_module_helpers/object-rest-spread-system-es5-i0-n0/crlfAstral/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0163 | `ge2_module_helpers/object-rest-spread-system-es5-i0-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0164 | `ge2_module_helpers/object-rest-spread-system-es5-i0-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0165 | `ge2_module_helpers/object-rest-spread-system-es5-i1-n0/original/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0166 | `ge2_module_helpers/object-rest-spread-system-es5-i1-n0/crlfAstral/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0167 | `ge2_module_helpers/object-rest-spread-system-es5-i1-n1/original/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0168 | `ge2_module_helpers/object-rest-spread-system-es5-i1-n1/crlfAstral/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0169 | `ge2_module_helpers/object-rest-spread-system-es2015-i0-n0/original/external` | `out/main.js.map` | 22:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0170 | `ge2_module_helpers/object-rest-spread-system-es2015-i0-n0/crlfAstral/external` | `out/main.js.map` | 22:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0171 | `ge2_module_helpers/object-rest-spread-system-es2015-i0-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0172 | `ge2_module_helpers/object-rest-spread-system-es2015-i0-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0173 | `ge2_module_helpers/object-rest-spread-system-es2015-i1-n0/original/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0174 | `ge2_module_helpers/object-rest-spread-system-es2015-i1-n0/crlfAstral/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0175 | `ge2_module_helpers/object-rest-spread-system-es2015-i1-n1/original/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0176 | `ge2_module_helpers/object-rest-spread-system-es2015-i1-n1/crlfAstral/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0177 | `ge2_module_helpers/object-rest-spread-system-es2017-i0-n0/original/external` | `out/main.js.map` | 22:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0178 | `ge2_module_helpers/object-rest-spread-system-es2017-i0-n0/crlfAstral/external` | `out/main.js.map` | 22:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0179 | `ge2_module_helpers/object-rest-spread-system-es2017-i0-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0180 | `ge2_module_helpers/object-rest-spread-system-es2017-i0-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0181 | `ge2_module_helpers/object-rest-spread-system-es2017-i1-n0/original/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0182 | `ge2_module_helpers/object-rest-spread-system-es2017-i1-n0/crlfAstral/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0183 | `ge2_module_helpers/object-rest-spread-system-es2017-i1-n1/original/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0184 | `ge2_module_helpers/object-rest-spread-system-es2017-i1-n1/crlfAstral/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0185 | `ge2_module_helpers/object-rest-spread-system-es2018-i0-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0186 | `ge2_module_helpers/object-rest-spread-system-es2018-i0-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0187 | `ge2_module_helpers/object-rest-spread-system-es2022-i0-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0188 | `ge2_module_helpers/object-rest-spread-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0189 | `ge2_module_helpers/iterable-bindings-forof-system-es2015-i0-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 6:1 | 通過 / 通過 |
| NATIVE-MAP-0190 | `ge2_module_helpers/iterable-bindings-forof-system-es2015-i0-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 6:1 | 通過 / 通過 |
| NATIVE-MAP-0191 | `ge2_module_helpers/iterable-bindings-forof-system-es2017-i0-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 6:1 | 通過 / 通過 |
| NATIVE-MAP-0192 | `ge2_module_helpers/iterable-bindings-forof-system-es2017-i0-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 6:1 | 通過 / 通過 |
| NATIVE-MAP-0193 | `ge2_module_helpers/iterable-bindings-forof-system-es2018-i0-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 6:1 | 通過 / 通過 |
| NATIVE-MAP-0194 | `ge2_module_helpers/iterable-bindings-forof-system-es2018-i0-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 6:1 | 通過 / 通過 |
| NATIVE-MAP-0195 | `ge2_module_helpers/iterable-bindings-forof-system-es2022-i0-n1/original/external` | `out/main.js.map` | 23:9 | `../main.ts` 6:1 | 通過 / 通過 |
| NATIVE-MAP-0196 | `ge2_module_helpers/iterable-bindings-forof-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 23:9 | `../main.ts` 6:1 | 通過 / 通過 |
| NATIVE-MAP-0197 | `ge2_module_helpers/sparse-array-call-spread-system-es5-i0-n0/original/external` | `out/main.js.map` | 20:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0198 | `ge2_module_helpers/sparse-array-call-spread-system-es5-i0-n0/crlfAstral/external` | `out/main.js.map` | 20:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0199 | `ge2_module_helpers/sparse-array-call-spread-system-es5-i0-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0200 | `ge2_module_helpers/sparse-array-call-spread-system-es5-i0-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0201 | `ge2_module_helpers/sparse-array-call-spread-system-es5-i1-n0/original/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0202 | `ge2_module_helpers/sparse-array-call-spread-system-es5-i1-n0/crlfAstral/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0203 | `ge2_module_helpers/sparse-array-call-spread-system-es5-i1-n1/original/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0204 | `ge2_module_helpers/sparse-array-call-spread-system-es5-i1-n1/crlfAstral/external` | `out/main.js.map` | 15:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0205 | `ge2_module_helpers/sparse-array-call-spread-system-es2015-i0-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0206 | `ge2_module_helpers/sparse-array-call-spread-system-es2015-i0-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0207 | `ge2_module_helpers/sparse-array-call-spread-system-es2015-i0-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0208 | `ge2_module_helpers/sparse-array-call-spread-system-es2015-i0-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0209 | `ge2_module_helpers/sparse-array-call-spread-system-es2015-i1-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0210 | `ge2_module_helpers/sparse-array-call-spread-system-es2015-i1-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0211 | `ge2_module_helpers/sparse-array-call-spread-system-es2015-i1-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0212 | `ge2_module_helpers/sparse-array-call-spread-system-es2015-i1-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0213 | `ge2_module_helpers/sparse-array-call-spread-system-es2017-i0-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0214 | `ge2_module_helpers/sparse-array-call-spread-system-es2017-i0-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0215 | `ge2_module_helpers/sparse-array-call-spread-system-es2017-i0-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0216 | `ge2_module_helpers/sparse-array-call-spread-system-es2017-i0-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0217 | `ge2_module_helpers/sparse-array-call-spread-system-es2017-i1-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0218 | `ge2_module_helpers/sparse-array-call-spread-system-es2017-i1-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0219 | `ge2_module_helpers/sparse-array-call-spread-system-es2017-i1-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0220 | `ge2_module_helpers/sparse-array-call-spread-system-es2017-i1-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0221 | `ge2_module_helpers/sparse-array-call-spread-system-es2018-i0-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0222 | `ge2_module_helpers/sparse-array-call-spread-system-es2018-i0-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0223 | `ge2_module_helpers/sparse-array-call-spread-system-es2018-i0-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0224 | `ge2_module_helpers/sparse-array-call-spread-system-es2018-i0-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0225 | `ge2_module_helpers/sparse-array-call-spread-system-es2018-i1-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0226 | `ge2_module_helpers/sparse-array-call-spread-system-es2018-i1-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0227 | `ge2_module_helpers/sparse-array-call-spread-system-es2018-i1-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0228 | `ge2_module_helpers/sparse-array-call-spread-system-es2018-i1-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0229 | `ge2_module_helpers/sparse-array-call-spread-system-es2022-i0-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0230 | `ge2_module_helpers/sparse-array-call-spread-system-es2022-i0-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0231 | `ge2_module_helpers/sparse-array-call-spread-system-es2022-i0-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0232 | `ge2_module_helpers/sparse-array-call-spread-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0233 | `ge2_module_helpers/sparse-array-call-spread-system-es2022-i1-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0234 | `ge2_module_helpers/sparse-array-call-spread-system-es2022-i1-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0235 | `ge2_module_helpers/sparse-array-call-spread-system-es2022-i1-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0236 | `ge2_module_helpers/sparse-array-call-spread-system-es2022-i1-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0237 | `ge2_module_helpers/tagged-template-system-es5-i0-n0/original/external` | `out/main.js.map` | 13:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0238 | `ge2_module_helpers/tagged-template-system-es5-i0-n0/crlfAstral/external` | `out/main.js.map` | 13:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0239 | `ge2_module_helpers/tagged-template-system-es5-i0-n1/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0240 | `ge2_module_helpers/tagged-template-system-es5-i0-n1/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0241 | `ge2_module_helpers/tagged-template-system-es5-i1-n0/original/external` | `out/main.js.map` | 13:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0242 | `ge2_module_helpers/tagged-template-system-es5-i1-n0/crlfAstral/external` | `out/main.js.map` | 13:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0243 | `ge2_module_helpers/tagged-template-system-es5-i1-n1/original/external` | `out/main.js.map` | 13:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0244 | `ge2_module_helpers/tagged-template-system-es5-i1-n1/crlfAstral/external` | `out/main.js.map` | 13:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0245 | `ge2_module_helpers/tagged-template-system-es2015-i0-n0/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0246 | `ge2_module_helpers/tagged-template-system-es2015-i0-n0/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0247 | `ge2_module_helpers/tagged-template-system-es2015-i0-n1/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0248 | `ge2_module_helpers/tagged-template-system-es2015-i0-n1/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0249 | `ge2_module_helpers/tagged-template-system-es2015-i1-n0/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0250 | `ge2_module_helpers/tagged-template-system-es2015-i1-n0/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0251 | `ge2_module_helpers/tagged-template-system-es2015-i1-n1/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0252 | `ge2_module_helpers/tagged-template-system-es2015-i1-n1/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0253 | `ge2_module_helpers/tagged-template-system-es2017-i0-n0/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0254 | `ge2_module_helpers/tagged-template-system-es2017-i0-n0/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0255 | `ge2_module_helpers/tagged-template-system-es2017-i0-n1/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0256 | `ge2_module_helpers/tagged-template-system-es2017-i0-n1/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0257 | `ge2_module_helpers/tagged-template-system-es2017-i1-n0/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0258 | `ge2_module_helpers/tagged-template-system-es2017-i1-n0/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0259 | `ge2_module_helpers/tagged-template-system-es2017-i1-n1/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0260 | `ge2_module_helpers/tagged-template-system-es2017-i1-n1/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0261 | `ge2_module_helpers/tagged-template-system-es2018-i0-n0/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0262 | `ge2_module_helpers/tagged-template-system-es2018-i0-n0/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0263 | `ge2_module_helpers/tagged-template-system-es2018-i0-n1/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0264 | `ge2_module_helpers/tagged-template-system-es2018-i0-n1/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0265 | `ge2_module_helpers/tagged-template-system-es2018-i1-n0/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0266 | `ge2_module_helpers/tagged-template-system-es2018-i1-n0/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0267 | `ge2_module_helpers/tagged-template-system-es2018-i1-n1/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0268 | `ge2_module_helpers/tagged-template-system-es2018-i1-n1/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0269 | `ge2_module_helpers/tagged-template-system-es2022-i0-n0/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0270 | `ge2_module_helpers/tagged-template-system-es2022-i0-n0/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0271 | `ge2_module_helpers/tagged-template-system-es2022-i0-n1/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0272 | `ge2_module_helpers/tagged-template-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0273 | `ge2_module_helpers/tagged-template-system-es2022-i1-n0/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0274 | `ge2_module_helpers/tagged-template-system-es2022-i1-n0/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0275 | `ge2_module_helpers/tagged-template-system-es2022-i1-n1/original/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0276 | `ge2_module_helpers/tagged-template-system-es2022-i1-n1/crlfAstral/external` | `out/main.js.map` | 9:9 | `../main.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0277 | `ge2_module_helpers/private-accessor-system-es2022-i0-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0278 | `ge2_module_helpers/private-accessor-system-es2022-i0-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0279 | `ge2_module_helpers/private-accessor-system-es2022-i0-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0280 | `ge2_module_helpers/private-accessor-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0281 | `ge2_module_helpers/private-accessor-system-es2022-i1-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0282 | `ge2_module_helpers/private-accessor-system-es2022-i1-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0283 | `ge2_module_helpers/private-accessor-system-es2022-i1-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0284 | `ge2_module_helpers/private-accessor-system-es2022-i1-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0285 | `ge2_module_helpers/standard-decorator-system-es5-i0-n0/original/external` | `out/main.js.map` | 68:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0286 | `ge2_module_helpers/standard-decorator-system-es5-i0-n0/crlfAstral/external` | `out/main.js.map` | 68:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0287 | `ge2_module_helpers/standard-decorator-system-es5-i0-n1/original/external` | `out/main.js.map` | 34:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0288 | `ge2_module_helpers/standard-decorator-system-es5-i0-n1/crlfAstral/external` | `out/main.js.map` | 34:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0289 | `ge2_module_helpers/standard-decorator-system-es5-i1-n0/original/external` | `out/main.js.map` | 38:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0290 | `ge2_module_helpers/standard-decorator-system-es5-i1-n0/crlfAstral/external` | `out/main.js.map` | 38:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0291 | `ge2_module_helpers/standard-decorator-system-es5-i1-n1/original/external` | `out/main.js.map` | 38:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0292 | `ge2_module_helpers/standard-decorator-system-es5-i1-n1/crlfAstral/external` | `out/main.js.map` | 38:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0293 | `ge2_module_helpers/standard-decorator-system-es2015-i0-n0/original/external` | `out/main.js.map` | 63:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0294 | `ge2_module_helpers/standard-decorator-system-es2015-i0-n0/crlfAstral/external` | `out/main.js.map` | 63:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0295 | `ge2_module_helpers/standard-decorator-system-es2015-i0-n1/original/external` | `out/main.js.map` | 29:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0296 | `ge2_module_helpers/standard-decorator-system-es2015-i0-n1/crlfAstral/external` | `out/main.js.map` | 29:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0297 | `ge2_module_helpers/standard-decorator-system-es2015-i1-n0/original/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0298 | `ge2_module_helpers/standard-decorator-system-es2015-i1-n0/crlfAstral/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0299 | `ge2_module_helpers/standard-decorator-system-es2015-i1-n1/original/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0300 | `ge2_module_helpers/standard-decorator-system-es2015-i1-n1/crlfAstral/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0301 | `ge2_module_helpers/standard-decorator-system-es2017-i0-n0/original/external` | `out/main.js.map` | 63:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0302 | `ge2_module_helpers/standard-decorator-system-es2017-i0-n0/crlfAstral/external` | `out/main.js.map` | 63:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0303 | `ge2_module_helpers/standard-decorator-system-es2017-i0-n1/original/external` | `out/main.js.map` | 29:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0304 | `ge2_module_helpers/standard-decorator-system-es2017-i0-n1/crlfAstral/external` | `out/main.js.map` | 29:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0305 | `ge2_module_helpers/standard-decorator-system-es2017-i1-n0/original/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0306 | `ge2_module_helpers/standard-decorator-system-es2017-i1-n0/crlfAstral/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0307 | `ge2_module_helpers/standard-decorator-system-es2017-i1-n1/original/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0308 | `ge2_module_helpers/standard-decorator-system-es2017-i1-n1/crlfAstral/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0309 | `ge2_module_helpers/standard-decorator-system-es2018-i0-n0/original/external` | `out/main.js.map` | 63:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0310 | `ge2_module_helpers/standard-decorator-system-es2018-i0-n0/crlfAstral/external` | `out/main.js.map` | 63:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0311 | `ge2_module_helpers/standard-decorator-system-es2018-i0-n1/original/external` | `out/main.js.map` | 29:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0312 | `ge2_module_helpers/standard-decorator-system-es2018-i0-n1/crlfAstral/external` | `out/main.js.map` | 29:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0313 | `ge2_module_helpers/standard-decorator-system-es2018-i1-n0/original/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0314 | `ge2_module_helpers/standard-decorator-system-es2018-i1-n0/crlfAstral/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0315 | `ge2_module_helpers/standard-decorator-system-es2018-i1-n1/original/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0316 | `ge2_module_helpers/standard-decorator-system-es2018-i1-n1/crlfAstral/external` | `out/main.js.map` | 33:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0317 | `ge2_module_helpers/standard-decorator-system-es2022-i0-n0/original/external` | `out/main.js.map` | 61:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0318 | `ge2_module_helpers/standard-decorator-system-es2022-i0-n0/crlfAstral/external` | `out/main.js.map` | 61:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0319 | `ge2_module_helpers/standard-decorator-system-es2022-i0-n1/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0320 | `ge2_module_helpers/standard-decorator-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0321 | `ge2_module_helpers/standard-decorator-system-es2022-i1-n0/original/external` | `out/main.js.map` | 31:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0322 | `ge2_module_helpers/standard-decorator-system-es2022-i1-n0/crlfAstral/external` | `out/main.js.map` | 31:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0323 | `ge2_module_helpers/standard-decorator-system-es2022-i1-n1/original/external` | `out/main.js.map` | 31:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0324 | `ge2_module_helpers/standard-decorator-system-es2022-i1-n1/crlfAstral/external` | `out/main.js.map` | 31:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0325 | `ge2_module_helpers/legacy-parameter-decorator-system-es5-i0-n0/original/external` | `out/main.js.map` | 36:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0326 | `ge2_module_helpers/legacy-parameter-decorator-system-es5-i0-n0/crlfAstral/external` | `out/main.js.map` | 36:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0327 | `ge2_module_helpers/legacy-parameter-decorator-system-es5-i0-n1/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0328 | `ge2_module_helpers/legacy-parameter-decorator-system-es5-i0-n1/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0329 | `ge2_module_helpers/legacy-parameter-decorator-system-es5-i1-n0/original/external` | `out/main.js.map` | 31:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0330 | `ge2_module_helpers/legacy-parameter-decorator-system-es5-i1-n0/crlfAstral/external` | `out/main.js.map` | 31:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0331 | `ge2_module_helpers/legacy-parameter-decorator-system-es5-i1-n1/original/external` | `out/main.js.map` | 31:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0332 | `ge2_module_helpers/legacy-parameter-decorator-system-es5-i1-n1/crlfAstral/external` | `out/main.js.map` | 31:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0333 | `ge2_module_helpers/legacy-parameter-decorator-system-es2015-i0-n0/original/external` | `out/main.js.map` | 29:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0334 | `ge2_module_helpers/legacy-parameter-decorator-system-es2015-i0-n0/crlfAstral/external` | `out/main.js.map` | 29:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0335 | `ge2_module_helpers/legacy-parameter-decorator-system-es2015-i0-n1/original/external` | `out/main.js.map` | 20:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0336 | `ge2_module_helpers/legacy-parameter-decorator-system-es2015-i0-n1/crlfAstral/external` | `out/main.js.map` | 20:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0337 | `ge2_module_helpers/legacy-parameter-decorator-system-es2015-i1-n0/original/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0338 | `ge2_module_helpers/legacy-parameter-decorator-system-es2015-i1-n0/crlfAstral/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0339 | `ge2_module_helpers/legacy-parameter-decorator-system-es2015-i1-n1/original/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0340 | `ge2_module_helpers/legacy-parameter-decorator-system-es2015-i1-n1/crlfAstral/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0341 | `ge2_module_helpers/legacy-parameter-decorator-system-es2017-i0-n0/original/external` | `out/main.js.map` | 29:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0342 | `ge2_module_helpers/legacy-parameter-decorator-system-es2017-i0-n0/crlfAstral/external` | `out/main.js.map` | 29:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0343 | `ge2_module_helpers/legacy-parameter-decorator-system-es2017-i0-n1/original/external` | `out/main.js.map` | 20:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0344 | `ge2_module_helpers/legacy-parameter-decorator-system-es2017-i0-n1/crlfAstral/external` | `out/main.js.map` | 20:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0345 | `ge2_module_helpers/legacy-parameter-decorator-system-es2017-i1-n0/original/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0346 | `ge2_module_helpers/legacy-parameter-decorator-system-es2017-i1-n0/crlfAstral/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0347 | `ge2_module_helpers/legacy-parameter-decorator-system-es2017-i1-n1/original/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0348 | `ge2_module_helpers/legacy-parameter-decorator-system-es2017-i1-n1/crlfAstral/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0349 | `ge2_module_helpers/legacy-parameter-decorator-system-es2018-i0-n0/original/external` | `out/main.js.map` | 29:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0350 | `ge2_module_helpers/legacy-parameter-decorator-system-es2018-i0-n0/crlfAstral/external` | `out/main.js.map` | 29:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0351 | `ge2_module_helpers/legacy-parameter-decorator-system-es2018-i0-n1/original/external` | `out/main.js.map` | 20:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0352 | `ge2_module_helpers/legacy-parameter-decorator-system-es2018-i0-n1/crlfAstral/external` | `out/main.js.map` | 20:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0353 | `ge2_module_helpers/legacy-parameter-decorator-system-es2018-i1-n0/original/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0354 | `ge2_module_helpers/legacy-parameter-decorator-system-es2018-i1-n0/crlfAstral/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0355 | `ge2_module_helpers/legacy-parameter-decorator-system-es2018-i1-n1/original/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0356 | `ge2_module_helpers/legacy-parameter-decorator-system-es2018-i1-n1/crlfAstral/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0357 | `ge2_module_helpers/legacy-parameter-decorator-system-es2022-i0-n0/original/external` | `out/main.js.map` | 29:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0358 | `ge2_module_helpers/legacy-parameter-decorator-system-es2022-i0-n0/crlfAstral/external` | `out/main.js.map` | 29:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0359 | `ge2_module_helpers/legacy-parameter-decorator-system-es2022-i0-n1/original/external` | `out/main.js.map` | 20:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0360 | `ge2_module_helpers/legacy-parameter-decorator-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 20:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0361 | `ge2_module_helpers/legacy-parameter-decorator-system-es2022-i1-n0/original/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0362 | `ge2_module_helpers/legacy-parameter-decorator-system-es2022-i1-n0/crlfAstral/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0363 | `ge2_module_helpers/legacy-parameter-decorator-system-es2022-i1-n1/original/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0364 | `ge2_module_helpers/legacy-parameter-decorator-system-es2022-i1-n1/crlfAstral/external` | `out/main.js.map` | 24:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0365 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n0/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0366 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n0/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0367 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n0/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0368 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n0/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0369 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n0/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0370 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n0/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0371 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n0/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0372 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n0/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0373 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n1/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0374 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n1/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0375 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n1/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0376 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n1/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0377 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n1/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0378 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n1/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0379 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n1/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0380 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i0-n1/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0381 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n0/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0382 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n0/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0383 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n0/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0384 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n0/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0385 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n0/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0386 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n0/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0387 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n0/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0388 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n0/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0389 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n1/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0390 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n1/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0391 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n1/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0392 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n1/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0393 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n1/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0394 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n1/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0395 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n1/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0396 | `ge2_module_helpers/interop-exportstar-cycle-system-es5-i1-n1/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0397 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n0/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0398 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n0/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0399 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n0/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0400 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n0/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0401 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n0/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0402 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n0/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0403 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n0/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0404 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n0/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0405 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n1/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0406 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n1/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0407 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n1/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0408 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n1/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0409 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n1/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0410 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n1/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0411 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n1/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0412 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i0-n1/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0413 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n0/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0414 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n0/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0415 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n0/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0416 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n0/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0417 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n0/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0418 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n0/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0419 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n0/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0420 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n0/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0421 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n1/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0422 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n1/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0423 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n1/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0424 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n1/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0425 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n1/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0426 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n1/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0427 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n1/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0428 | `ge2_module_helpers/interop-exportstar-cycle-system-es2015-i1-n1/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0429 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n0/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0430 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n0/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0431 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n0/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0432 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n0/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0433 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n0/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0434 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n0/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0435 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n0/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0436 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n0/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0437 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n1/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0438 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n1/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0439 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n1/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0440 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n1/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0441 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n1/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0442 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n1/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0443 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n1/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0444 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i0-n1/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0445 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n0/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0446 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n0/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0447 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n0/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0448 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n0/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0449 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n0/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0450 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n0/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0451 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n0/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0452 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n0/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0453 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n1/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0454 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n1/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0455 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n1/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0456 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n1/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0457 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n1/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0458 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n1/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0459 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n1/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0460 | `ge2_module_helpers/interop-exportstar-cycle-system-es2017-i1-n1/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0461 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n0/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0462 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n0/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0463 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n0/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0464 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n0/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0465 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n0/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0466 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n0/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0467 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n0/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0468 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n0/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0469 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n1/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0470 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n1/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0471 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n1/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0472 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n1/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0473 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n1/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0474 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n1/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0475 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n1/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0476 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i0-n1/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0477 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n0/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0478 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n0/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0479 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n0/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0480 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n0/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0481 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n0/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0482 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n0/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0483 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n0/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0484 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n0/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0485 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n1/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0486 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n1/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0487 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n1/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0488 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n1/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0489 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n1/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0490 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n1/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0491 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n1/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0492 | `ge2_module_helpers/interop-exportstar-cycle-system-es2018-i1-n1/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0493 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n0/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0494 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n0/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0495 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n0/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0496 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n0/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0497 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n0/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0498 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n0/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0499 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n0/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0500 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n0/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0501 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n1/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0502 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n1/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0503 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n1/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0504 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n1/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0505 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n1/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0506 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n1/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0507 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0508 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i0-n1/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0509 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n0/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0510 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n0/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0511 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n0/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0512 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n0/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0513 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n0/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0514 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n0/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0515 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n0/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0516 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n0/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0517 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n1/original/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0518 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n1/original/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0519 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n1/original/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0520 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n1/original/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0521 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n1/crlfAstral/external` | `out/dep.js.map` | 10:9 | `../dep.ts` 2:1 | 通過 / 通過 |
| NATIVE-MAP-0522 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n1/crlfAstral/external` | `out/left.js.map` | 16:9 | `../left.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0523 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n1/crlfAstral/external` | `out/main.js.map` | 27:9 | `../main.ts` 5:1 | 通過 / 通過 |
| NATIVE-MAP-0524 | `ge2_module_helpers/interop-exportstar-cycle-system-es2022-i1-n1/crlfAstral/external` | `out/right.js.map` | 14:9 | `../right.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0525 | `ge2_module_helpers/optional-nullish-system-es2022-i0-n0/original/external` | `out/main.js.map` | 10:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0526 | `ge2_module_helpers/optional-nullish-system-es2022-i0-n0/crlfAstral/external` | `out/main.js.map` | 10:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0527 | `ge2_module_helpers/optional-nullish-system-es2022-i0-n1/original/external` | `out/main.js.map` | 10:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0528 | `ge2_module_helpers/optional-nullish-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 10:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0529 | `ge2_module_helpers/optional-nullish-system-es2022-i1-n0/original/external` | `out/main.js.map` | 10:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0530 | `ge2_module_helpers/optional-nullish-system-es2022-i1-n0/crlfAstral/external` | `out/main.js.map` | 10:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0531 | `ge2_module_helpers/optional-nullish-system-es2022-i1-n1/original/external` | `out/main.js.map` | 10:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0532 | `ge2_module_helpers/optional-nullish-system-es2022-i1-n1/crlfAstral/external` | `out/main.js.map` | 10:9 | `../main.ts` 3:1 | 通過 / 通過 |
| NATIVE-MAP-0533 | `ge2_module_helpers/helper-name-collision-system-es5-i0-n0/original/external` | `out/main.js.map` | 54:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0534 | `ge2_module_helpers/helper-name-collision-system-es5-i0-n0/crlfAstral/external` | `out/main.js.map` | 54:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0535 | `ge2_module_helpers/helper-name-collision-system-es5-i0-n1/original/external` | `out/main.js.map` | 18:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0536 | `ge2_module_helpers/helper-name-collision-system-es5-i0-n1/crlfAstral/external` | `out/main.js.map` | 18:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0537 | `ge2_module_helpers/helper-name-collision-system-es5-i1-n0/original/external` | `out/main.js.map` | 22:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0538 | `ge2_module_helpers/helper-name-collision-system-es5-i1-n0/crlfAstral/external` | `out/main.js.map` | 22:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0539 | `ge2_module_helpers/helper-name-collision-system-es5-i1-n1/original/external` | `out/main.js.map` | 22:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0540 | `ge2_module_helpers/helper-name-collision-system-es5-i1-n1/crlfAstral/external` | `out/main.js.map` | 22:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0541 | `ge2_module_helpers/helper-name-collision-system-es2015-i0-n0/original/external` | `out/main.js.map` | 22:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0542 | `ge2_module_helpers/helper-name-collision-system-es2015-i0-n0/crlfAstral/external` | `out/main.js.map` | 22:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0543 | `ge2_module_helpers/helper-name-collision-system-es2015-i0-n1/original/external` | `out/main.js.map` | 13:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0544 | `ge2_module_helpers/helper-name-collision-system-es2015-i0-n1/crlfAstral/external` | `out/main.js.map` | 13:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0545 | `ge2_module_helpers/helper-name-collision-system-es2015-i1-n0/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0546 | `ge2_module_helpers/helper-name-collision-system-es2015-i1-n0/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0547 | `ge2_module_helpers/helper-name-collision-system-es2015-i1-n1/original/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0548 | `ge2_module_helpers/helper-name-collision-system-es2015-i1-n1/crlfAstral/external` | `out/main.js.map` | 17:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0549 | `ge2_module_helpers/helper-name-collision-system-es2017-i0-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0550 | `ge2_module_helpers/helper-name-collision-system-es2017-i0-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0551 | `ge2_module_helpers/helper-name-collision-system-es2017-i0-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0552 | `ge2_module_helpers/helper-name-collision-system-es2017-i0-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0553 | `ge2_module_helpers/helper-name-collision-system-es2017-i1-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0554 | `ge2_module_helpers/helper-name-collision-system-es2017-i1-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0555 | `ge2_module_helpers/helper-name-collision-system-es2017-i1-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0556 | `ge2_module_helpers/helper-name-collision-system-es2017-i1-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0557 | `ge2_module_helpers/helper-name-collision-system-es2018-i0-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0558 | `ge2_module_helpers/helper-name-collision-system-es2018-i0-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0559 | `ge2_module_helpers/helper-name-collision-system-es2018-i0-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0560 | `ge2_module_helpers/helper-name-collision-system-es2018-i0-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0561 | `ge2_module_helpers/helper-name-collision-system-es2018-i1-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0562 | `ge2_module_helpers/helper-name-collision-system-es2018-i1-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0563 | `ge2_module_helpers/helper-name-collision-system-es2018-i1-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0564 | `ge2_module_helpers/helper-name-collision-system-es2018-i1-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0565 | `ge2_module_helpers/helper-name-collision-system-es2022-i0-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0566 | `ge2_module_helpers/helper-name-collision-system-es2022-i0-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0567 | `ge2_module_helpers/helper-name-collision-system-es2022-i0-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0568 | `ge2_module_helpers/helper-name-collision-system-es2022-i0-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0569 | `ge2_module_helpers/helper-name-collision-system-es2022-i1-n0/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0570 | `ge2_module_helpers/helper-name-collision-system-es2022-i1-n0/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0571 | `ge2_module_helpers/helper-name-collision-system-es2022-i1-n1/original/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |
| NATIVE-MAP-0572 | `ge2_module_helpers/helper-name-collision-system-es2022-i1-n1/crlfAstral/external` | `out/main.js.map` | 11:9 | `../main.ts` 4:1 | 通過 / 通過 |

## 重跑

先將 `BLUEICE_BLUETSC_ORACLE` 設為原版 TypeScript 5.9.3 的 `bin/tsc`，然後執行：

```sh
node --test development/browser_core/phase-18-bluets/tools/ge5_maps.test.cjs development/browser_core/phase-18-bluets/tools/ge5_native_patch.test.cjs
node development/browser_core/phase-18-bluets/tools/verify_ge5_native_patch.cjs --all
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test -p blueice-bluets --test ge5_source_maps native_defects -- --nocapture
```

一般重跑只核對 ledger，不改寫它。`--write` 僅供明確產生獨立修補證據；stock reference 沒有寫入路徑。

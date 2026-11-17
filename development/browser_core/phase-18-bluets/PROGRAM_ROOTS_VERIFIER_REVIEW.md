# P0-H 驗收器修正審查

狀態：**使用者已於2026-11-17核准六組修正；14個原始工具檔已按審查雜湊套用，正式驗收執行中。**
本文件彙整可審查的差異，避免把編譯器缺陷、驗收器缺陷與預覽結果混在一起。
目前已整合編譯器為 source59/cache124，backend tree 為
`37beae676a55482728b6aa8682240bd9588e7fdc`。
下列 source58 預覽保留其原來源綁定，不能作為 source59 的正式驗收結果。

核准原文：「核准套用六組驗收器修正」。六份patch與14個檔案雜湊已逐一核對；
套用當時其他source／fixture與真實Git index未變。實際套用紀錄與618工具雜湊位於
`target/approved-verifiers-source59-20261117-1/application-result.json` 及
`tools-after.json`。先前預覽保留原時點的來源與未核准狀態，不改寫歷史證據。

正式Native驗收已完成191 suites／1,088 tests，零失敗、ignored或filtered；
55個原始map套件／298個名稱與原始log雜湊也已獨立核對。後續工具維護只更新
兩份單元測試斷言：symlink清單明列新增的兩項Unix案例，另將既有hidden-grant
負向控制的預期錯誤訊息改為已核准檢查器的精確訊息。變異與拒絕檢查保留，
scanner、25項Windows矩陣與Native量測JSON均未修改。47份Node測試現為304／304
通過、零略過；最終618工具雜湊另存於 `inventory-and-tooling-repair/tools-after.json`，
不改寫Native執行當時的工具收據。完整workspace／其map補測及最終交付稽核仍待完成。

## 六組修正

| 提案 | 原問題與具體修正 | 保留的驗收約束 |
| --- | --- | --- |
| 1. Helper transition 順序 | 完整 map 相等檢查先攔截了刻意破壞的 transition，導致負向控制無法確認應觸發的邊界。先檢查座標與同位置 transition，再做完整相等比較。 | 原完整 map 相等檢查保留；衝突 owner 仍拒絕。 |
| 2. Iterator initializer gap | for-of binding 前綴和 initializer 都有檢查，但兩者間的 gap 未比較。補上完整 gap 比較。 | 原 token、initializer、loop body 與 generated no-source 檢查全部保留。 |
| 3. No-map EOF | 移除 sourceMappingURL 時錯把 JavaScript 本體最後的換行一起移除。擷取本體時保留該換行，再比較完整原始位元組。 | 66 個合法 no-map 設定與原本非法 sourceRoot 設定的拒絕各自保留；合法投影另列，不改成直接相容。 |
| 4. Decorator phase permutation | 原負向控制交換多行 phase 時沒有同步移動完整 map，會先產生無效控制。交換完整相鄰 statement，並投影每一個原 map point。 | 控制必須維持合法 JavaScript、完整 metadata、相同 source-role 多重集合；每個 owner 保留一次。 |
| 5. Continuation action selector | 用 `kind:"finish"` 字串尋找 AST，受格式化空白影響。改檢查 return/object/property/string-literal 結構。 | 原 leave/finish 語義、負向變異及其拒絕檢查保留。 |
| 6. EOF／Native factory／重合座標 | EOF 負向變異不得新增越界 restore；辨識原始 class 的 Native factory；重合座標只能具有該座標的完整、依序 Native owner 群組。 | 原完整 module、declaration、map、runtime 檢查保留；相同生成文字必須具有完整 Native 重合群組，publication endpoint 也需相符。 |

所有提案均保留原輸入、Native fixture、case ID 與分類。原有 618 個工具、
12,201 個 fixture、43,112 個 assertion ID／分類與 572 項既有修正的回歸要求
不因這份提案減少。原始失敗輸出也保留；預覽不能取代正式通過紀錄。

## 可重現差異與證據

下列路徑均相對於 repository root；`target/` 內容是本機審查證據。

| 提案 | Patch 路徑（位於 `target/retained-method-maps-20261115-1/`） | 已保存的獨立預覽 |
| --- | --- | --- |
| 1 | `verifier-order-review/proposal.patch` | source51：363 configurations、25,972 controls。 |
| 2 | `verifier-order-review/iteration-gap.proposal.patch` | source51：339 maps、31,465 controls；原 75 個 gap 失敗保留。 |
| 3 | `verifier-order-review/replacement-nomap-eof.proposal.patch` | Native self-calibration 與歷史 source43 完整 67 outputs。 |
| 4 | `verifier-order-review/decorator-phase/proposal.patch` | 歷史四個樣本、4,586 controls；不足以當作目前完整 family 通過。 |
| 5 | `verifier-order-review/continuation-actions/proposal.patch` | 歷史 28 products、1,645 controls；原四個失敗保留。 |
| 6 | `local-string-export-bounds-diagnostic-1/review2/proposal.patch` | source58：120 configurations、180 maps、1,080 artifacts、240 executions、23,601 原 controls；另九項拒絕控制全部通過。 |

第六組的補充控制包含隱藏 owner grant、owner 順序交換、額外重合 owner、
刪除／搬移 Native event、EOF／最後字元 retarget、越界變異與 constructor 改名。
其 `PROOF.json`、`negative-controls.json`、`REVIEW.md` 與完整報告均在相同目錄。
上述歷史預覽各自綁定原測量的 compiler/source，不當作 source58 的正式結果。

新增 source59 隔離證據：member-keyword 的 producer 修正使原200個 non-ES5
generator 產品通過原驗收器。兩個 ES5 finally family 的原失敗因此收斂為
`assert(finish)` selector 問題；只載入第5組提案的隔離預覽後，兩個完整 family
共14 maps通過。原失敗保留，原工具未修改，也不給正式驗收 credit。
證據在 `target/source58-async-generator-regression-20261116-1/proposal5-current59-preview-1/`。

六組可在隔離副本中依序套用，合併結果共變更 14 個工具檔。
`target/source58-combined-verifier-preview-20261116-1/manifest.json` 記錄每份 patch、
原始工具與提議工具的 SHA-256。預覽以原檔名載入已記錄的提議內容，另記實際
執行內容的 hash；原始磁碟工具不變。Parent 與 worker 的載入一致性已驗證。
完整原 55 個 family（含 ignored tests）的合併預覽位於 `all55-preview/`，
實際 Cargo CLI SHA-256 為
`e5a3c782593d84410b086b81324e0b205095a7fdb5e55403ca84d9d947795cdf`。
該工作已結束，結果為 **53 通過／2 失敗，且不給正式驗收 credit**。
兩個失敗 family 的 member-keyword producer 問題由 source59 修正；
ES5 finally selector 的第5組提案仍未套用。仍須以核准後的實際工具完成正式重跑。

## Source59 完整55組預覽與獨立核對

source59 的實際 Cargo workspace harness 已完整通過原55組／298項測試，
其中 `ge5_source_maps` 全64項通過；零失敗、忽略或篩選。實際 compiler SHA
與下列120設定重跑相同。獨立核對逐組原始 log、compiled catalogue及舊
source58的完整測試名稱，並核對55組加上補充local-export的146份載入trace／
478筆事件，確認14個提案工具的實際執行SHA全部相符。Source、原始工具／
fixture與真實Git index保持不變。證據：
`target/source59-combined-verifier-preview-20261116-2/all55-preview/independent-preview-verification.json`。
這仍是記憶體載入提案的診斷預覽；不給正式驗收credit，也不代表已獲核准。

## Source59 補充的完整 local-string-export 預覽

原始 `ge5_local_string_exports` 不在55個 map family 清單內，因此另以
source59 實際 Cargo workspace 測試執行檔完整重跑。compiler SHA-256 為
`c54c2d25140652631eb0e81e76804a5af16011b6404b90a53b7503873e2f7466`。
原始測試的120 configurations、180 maps、1,080 artifacts、240 executions
及23,601原有 controls 全部通過，source、原工具／fixture與實際 Git index
的雜湊均核對未變。原始工具未修改；提案僅在記憶體載入，沒有正式驗收 credit。
證據位於 `target/source59-local-string-export-preview-20261116-1/evidence/`；
`result.json` 與完整 report 分別保存實際 harness、compiler及輸出雜湊。
前述九項補充控制的原 source58 證據保留。2026-11-17另使用已保存的source59
artifacts（compiler SHA同上），直接載入已核准的磁碟工具重跑：9/9拒絕控制、
actual與Native-self正向控制全部通過，沒有preload或記憶體替換。證據位於
`target/approved-verifiers-source59-20261117-1/negative-controls/negative-controls.json`。
此項綁定既存source59 artifacts，不取代正在執行的正式Native191量測。
此項通過不替代核准後的完整55組及正式 workspace 驗收。

## 核准後的正式步驟

來源綁定分為兩部分：`compatibility_inventory/snapshot.rs` 的四個
`SOURCE_ROOTS` 都在 backend 編譯器／測試目錄，phase Node 工具另由正式 runner
的 `toolHashes()` 保存。因此相同 backend tree 不代表相同驗收工具；套用核准
修正後，必須重新鎖定全部618個實際工具 hash，並在正式執行前後核對。
`target/program-roots-source59-acceptance-20261116-1/formal-tools-review/`
已唯讀核對六份 patch 與14個原始／提議檔案 hash，保存目前及提議工具清單。
提議清單仍標示未核准、非正式驗收；不得拿來冒充實際執行的工具清單。

明確核准已收到並完成套用。接著以實際工具位元組
重跑完整 Native 191 suites、原 55 map family、兩個編譯器 profile 的
8,407 configurations × 兩次 build，以及受影響的完整 workspace 驗收。
不得以預覽 preload 產生正式通過紀錄，也不得減少原 denominator 或改動 case 分類。

完整 P0-H 還須具備同來源的 Windows 71 cohorts／82 oracle／25 path、
program-root、project、tooling 與獨立 accounting 證據，才可勾選、commit 並 push。
核准來自上述使用者明確指示；正式驗收尚未完成，本文件不代表 P0-H 完成。

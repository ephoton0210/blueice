# about:assistant 頁面。任何訊息都不帶參數:Fluent 會在參數前後加上雙向隔離
# 字元,顯示在網頁文字裡會變成怪字,所以由頁面自行組合數值與標籤。
assistant-title = 助理
assistant-empty = 目前沒有內容。請求某個頁面的摘要,或請助理整理其中的資料,結果會顯示在這裡。
assistant-unavailable = 尚未設定本機助理,因此無法提供摘要與資料整理。
assistant-note = 由本機模型根據頁面文字撰寫。重要細節請與頁面本身核對。
kind-summary = 摘要
kind-organized = 整理後的資料
label-source = 頁面:
label-request = 請求:
label-failed = 無法完成這項請求:

# about:assistant 的設定區段:任何訊息都不帶參數(見 assistant.ftl 開頭的說明),
# 由頁面自行組合標籤與數值。
settings-heading = 設定
settings-none = 未指定設定檔,助理只依啟動選項運作。
settings-missing = 設定檔不存在,因此沒有設定任何助理。
settings-invalid = 無法使用設定檔:
settings-file = 檔案:
settings-apply = 對此檔案的修改會在啟動器下次啟動時生效。
settings-backend = 後端:
backend-none = 無(未設定助理)
backend-loopback = 本機伺服器
backend-candle = 行程內(candle)
backend-both = 兩者同時(資源加倍)
settings-loopback = 本機模型:
settings-candle-model = Candle 模型:
settings-candle-tokenizer = Candle 分詞器:
settings-candle-context = Candle 上下文(token 數):
settings-idle = 閒置逾時(秒):
settings-memory = 記憶體上限(MiB):
settings-memory-none = 不限制
settings-nice = 優先權(nice):

# Multi-format backends — handover prompts

One prompt per phase, designed to be pasted as the first message of a fresh session in
`~/repos/confy`. Run them **in order**; each phase assumes the previous one is merged
to `main`. Phase 1 has a ready code-level plan; Phases 2–4 deliberately do **not** —
their detailed plans must be written in-session against the then-current codebase
(Phase 2–4 interfaces depend on Phase 1's refactor; the YAML plan depends on the spike
outcome), so each prompt is a plan-then-implement instruction.

Canonical spec for all phases:
`docs/superpowers/specs/2026-06-12-multiformat-backends-design.md`

---

## Phase 1 — backend abstraction (refactor only)

```
/wens-plan-implementer

執行 docs/superpowers/plans/2026-06-12-phase1-backend-abstraction.md（Phase 1：後端抽象
化重構，spec 在 docs/superpowers/specs/2026-06-12-multiformat-backends-design.md §Phase 1）。

要點：
- 純重構，不出新格式：AnyDocument enum 包裝、Mutation 欄位 toml:→fragment:、
  kind_options 能力查詢、DocFormat/comment_prefix facets、CLI 認得 json/yaml 副檔名但
  bail。
- 驗收閘門：現有測試套件除機械式改名外一字不改全數通過；每個 task 後
  cargo test && cargo clippy -- -D warnings && cargo fmt --check 全綠。
- plan 裡引用的行號是規劃時的快照，動手前先用 rg 重新定位。
- 完成後依 CLAUDE.md 規範更新 CHANGELOG.md / CLAUDE.md。
```

（不用 agd 流程時，改成：`執行 docs/superpowers/plans/2026-06-12-phase1-backend-abstraction.md，
用 superpowers:executing-plans 逐 task 實作`，其餘要點相同。）

---

## Phase 2 — JSON/JSONC backend

```
為 confy 實作 JSON/JSONC 後端（spec：docs/superpowers/specs/
2026-06-12-multiformat-backends-design.md §Phase 2，先整份讀完，特別是 2.2 投影映射表、
2.3 行為矩陣、2.4 KIND 欄）。Phase 1（AnyDocument/kind_options/DocFormat）已在 main。

流程：先用 writing-plans skill 依 spec §Phase 2 寫出 code 層級的實作 plan（存
docs/superpowers/plans/）給我核准，再逐 task 實作。

規劃時的硬約束（spec 已定案，不要重開）：
- lossless：未動過的檔案 serialize() byte-identical；自寫 rowan CST（直接依賴 rowan，
  pin taplo 用的同一版本），src/model/json/ 六檔結構鏡像 cst_* 三件套的慣用法
  （clone_for_update 原子 commit、validate 後備檢查、golden projection 測試）。
- JSONC：// 行註解＝comment 節點、行尾 //＝trailing_comment、/* */ 解析保留但唯讀
  （Node 新增 read_only flag）；trailing comma 解析接受、自家 splice 不產生。
- 新增 ScalarType::Null、Format::Exponent、KindTarget::TableMultiline、KIND tag
  [S:null]/[T/M]；type_filter 的 classify 與 type_tag 同步擴充並保住互為反函數的
  invariant 測試；f popup 的 facet 集依 DocFormat 篩選。
- 純 .json 首次引入註解 → Mode::Prompt(JsoncUpgrade) 確認（沿用 ArrayUpgrade 模式）；
  r remark 寫成 // "key": value,。
- help 選單出 JSON 版（keys.rs 的 help_text(DocFormat) 已留 match 臂）。
- 測試：golden projection、tests/fixtures/*.json(c) byte-identical roundtrip、
  mutation 單元測試鏡像 cst_edit 套路;完成後更新 CHANGELOG/CLAUDE.md/CONTEXT.md/README。
- 不准用 pty 驅動 TUI 測試；真機驗證留給我手動。
```

---

## Phase 3 — YAML subset backend

```
為 confy 實作 YAML 子集後端（spec：docs/superpowers/specs/
2026-06-12-multiformat-backends-design.md §Phase 3，先整份讀完）。Phase 1–2 已在 main，
src/model/json/ 是後端結構的第二個範本。

第一步是 spec 3.3 的【parser spike 閘門】，這步沒過就停下回報、不要硬上：
- 自寫 lossless YAML 子集 lexer/parser（rowan、INDENT token 進 token 流），收 ≥10 個
  真實檔案語料（docker-compose 含 anchor、GitHub Actions、k8s manifest、簡單 config），
  驗 parse 成功＋serialize byte-identical＋子集外構造正確圍成 opaque 唯讀節點。
- spike 過了，再用 writing-plans skill 寫整個 phase 的 code 層級 plan 給我核准，
  然後逐 task 實作。

規劃時的硬約束（spec 已定案）：
- 子集＝單一 document、block/flow map+seq、五種 scalar style（plain/single/double/|/>
  含 chomping）、# 註解；scalar 型別走 YAML 1.2 core schema（無 datetime）。
- anchor/alias/merge-key/tag/多行 flow → opaque 唯讀節點（read_only flag，Phase 2 已建），
  mutation 一律 Unsupported、文件不動；multi-document 檔載入即拒。
- splice 層核心是 indent engine：插入/搬移的 fragment 整段重縮排到目的深度。
- Format 新變體 Block/SingleQuoted/DoubleQuoted/LiteralBlock/Folded、KindTarget 新變體
  Flow/Block/StringPlain/StringSingle/StringDouble/StringLiteralBlock/StringFolded；
  KIND tag [T/B]/[T/F]/[A/B]/[A/F]/[opaq ]；classify↔type_tag invariant 照舊擴充。
- 行為矩陣依 spec 3.4（keyed fragment 進 sequence 變 `- ` block mapping；r remark 用 #；
  K 的 block↔flow 與五種 string style 互轉及其拒絕規則）。
- 測試與文件要求同 Phase 2；不准用 pty 驅動 TUI 測試。
```

---

## Phase 4 — document-level conversion

```
為 confy 實作文件級格式轉換（spec：docs/superpowers/specs/
2026-06-12-multiformat-backends-design.md §Phase 4，先整份讀完，特別是 4.3 損失與合法性
矩陣）。Phase 1–3 已在 main（三個後端都能編輯）。

流程：先用 writing-plans skill 依 spec §Phase 4 寫 plan 給我核准，再實作。

規劃時的硬約束（spec 已定案）：
- 新 src/model/value.rs（Value enum，含 ordered map 與 per-node 前置/行尾註解）＋
  src/model/convert.rs；每後端實作 to_value() 與 render_value()（預設風格序列化）。
- CLI：confy convert <in> <out>（副檔名定格式、--from/--to 覆寫、先列 lossy 警告、
  --yes 或 TTY y/n 確認）；TUI：Root 節點上的轉換操作（鍵位實作時定，help 要列）。
- 損失矩陣照 4.3：註解在目標支援時保留（#↔//）；記法/風格正規化要警告；
  TOML datetime → string 要警告；null → TOML 直接 abort 並列出所有 null 路徑；
  來源含 YAML opaque 節點 → abort；abort 不寫出任何檔案、來源檔永不被 convert 修改。
- 測試：矩陣每列、abort 案例、註解搬運、CLI 整合測試（assert_cmd）；
  完成後更新 CHANGELOG/CLAUDE.md/README（格式支援表）。
```

# 私有持仓：一次读取、一次更新、一次验收

不联网、不调用模型、不发送委托。Python 3 标准库即可；PowerShell 脚本可用 `-Python` 指定解释器。默认使用 Codex 的本机 Python，未安装则明确报错。

```powershell
.\scripts\portfolio.ps1 -Command inspect
.\scripts\portfolio.ps1 -Command update -InputFile C:\private\batch.json -Restart
.\scripts\portfolio.ps1 -Command validate
.\scripts\portfolio.ps1 -Command export
```

`inspect` 返回当前账户和 `portfolio_hash`。将摘要原样填入 batch 的 `expected_hash`，按券商账号尾号更新；每个出现的账户必须提供**完整最终持仓**和截图可用资金，未出现账户不变。只有确认完整清仓时才给空数组，零股行不属于当前持仓。

示例仅使用虚构账户：

```json
{
  "expected_hash": "从 inspect 复制",
  "observed_at": "2026-09-01T14:32:00+08:00",
  "accounts": [{
    "suffix": "1234",
    "cash": 9000.0,
    "holdings": [{
      "code": "600000", "name": "示例", "market": "Shanghai",
      "quantity": 100.0, "cost_price": 10.0,
      "available_quantity": 0.0, "available_date": "2026-09-01",
      "intraday_cost_price": 10.0
    }]
  }]
}
```

`observed_at` 是观察时间，不冒充成交时间。`available_date` 是该可用数量对应的交易日，不是解禁日；当天买入的成本不清楚时不要填 `intraday_cost_price`。截图未给日期时先确认日期，不能用文件修改时间推断。

默认只生成数量差额的“截图推断/待核”记录。买入价格最多是成本推算；卖价不知道则保留未知。不要用其他账户现价代替成交价，不要把现金差额算成手续费、转账或已实现收益。

有真实成交单时可提供 `transactions`（与应用 Transaction 字段一致），包括唯一 id、account_id、date、kind、code/name/market、quantity、price、fees、cash_amount、realized_pnl、note 和 execution。`execution` 包含 confidence、price_basis、fees_known、date_is_estimated、executed_at、observed_at；确实知道才用 Confirmed/fill，未知盈亏为 null。成交净数量必须解释该账户最终持仓变化。

## 校验和恢复

- 更新器关闭**固定路径**的程序，创建时间戳备份，再在共享锁内更新；检测到其他路径的旧程序会停止。
- 过时摘要、重复交易 ID、非法类型、可用数量超限、缺失账本和现金不一致均报错；失败不宣称成功。
- 一次保存写入当前状态、历史证据和派生账本，普通写入异常回滚。断电/强杀造成中断时，先检查锁与同批备份，再恢复或重建；不提供虚假的跨文件硬件事务保证。
- `validate` 把结构一致与成交是否核实分开报告；`execution_reconciled: false` 表示仍有历史待核，不等于文件损坏。
- 应用内交易后 `rebuild` 更新派生账本。不会再根据文件修改时间重复推断历史交易。
- `export` 生成 Excel 可打开的 UTF-8 CSV；`-OutputFile` 指定位置。批次 JSON、CSV、截图和备份全部放在仓库外。

仅首次迁移旧账本使用 `-Command migrate`；恢复时使用同一备份目录中的整套文件。`-AppDir` 可用于临时隔离验证。

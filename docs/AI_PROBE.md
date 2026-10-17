# AI Data Plane 与单次 Probe

当前版本在已校验的 `ra3_1.12.game` 内维护一个只读、固定容量的 AI data plane。它暂时只提供 CLI，不注册 MCP 工具。

## 执行边界

- ASI 在 `SkirmishAI::Update`（VA `0x00909A90`，RVA `0x00509A90`）返回后采样。
- 游戏逻辑线程只读取已经确认的对象偏移，并把结果复制到进程内固定容量快照；不分配内存、不访问文件或管道。
- 发布使用 `try_lock`。CLI 导出线程恰好占用 plane 时，本帧采样直接跳过，并累计 `skipped_captures`，游戏线程不等待。
- 独立管道工作线程从 plane 复制稳定快照，再以二进制协议发送。JSON 编码和文件写入全部发生在外部 `ra3ctl.exe`。
- Plane 最多保留 16 个最近出现的 `SkirmishAI` 实例；每个实例最多导出 64 个战略状态。超过上限时设置截断标志，原始总数仍保留。

鼠标输入管道 `\\.\pipe\RA3MCP.<PID>` 及其 24 字节 v1 协议保持不变。AI 快照使用独立端点：

```text
\\.\pipe\RA3MCP.AI.<PID>
```

## CLI

进入一局包含 Skirmish AI 的游戏，取得 `ra3_1.12.game` PID 后执行：

```powershell
$gamePid = (Get-CimInstance Win32_Process -Filter "Name='ra3_1.12.game'").ProcessId
.\target\release\ra3ctl.exe --pid $gamePid probe
```

默认输出到当前目录的 `dumps/`：

```text
dumps/ra3-ai-probe-<PID>-<UTC>-g<plane generation>.json
```

也可指定完整文件名；已有文件不会被覆盖：

```powershell
.\target\release\ra3ctl.exe --pid $gamePid probe --output .\dumps\manual-ai-probe.json
```

成功输出示例：

```text
dumped agents=3 generation=1842 captured=1842 skipped=0 file=dumps\ra3-ai-probe-39836-...json
```

刚进入游戏、尚未执行过 `SkirmishAI::Update` 时，CLI 会返回 `not observed a SkirmishAI update yet`，不会生成空文件。

## 首版导出字段

每份 JSON 同时携带导出时间 `exported_utc`、毫秒 Unix 时间、plane generation、跳过的采样数，以及每个 AI 实例自己的采集时间和游戏帧。

每个 AI 实例当前导出：

- `SkirmishAI`、Player、ArmyDefinition、PersonalityRef/Definition 地址。
- Bank、BuildManager、BuildStateManager、StrategicStateManager、StrategyManager、EconomyManager 地址。
- disabled、last update frame、timer、strategic directive。
- StrategyManager enabled、interval、next frame。
- Personality update interval，以及 compatible factions、weighted structures、strategic/budget/build definition 列表的数量与地址。
- 战略状态 vector 的 begin/end/capacity、总数、容量、下一评估帧。
- 每个已导出战略状态的 definition 地址、归一化 score、原始 float bits 和运行标志。

地址用于本轮探针和后续资产解析，不应跨进程或跨局缓存成稳定 ID。后续接入 LLM 前，应把 definition 地址映射为稳定的资产 ID 或名称。

## AI 快照协议 v1

所有整数均为小端。请求固定 16 字节：

| 偏移 | 类型 | 字段 |
| --- | --- | --- |
| 0 | u32 | magic `0x49413352`，ASCII `R3AI` |
| 4 | u16 | version `1` |
| 6 | u16 | operation `0`：读取最新 plane |
| 8 | u32 | sequence |
| 12 | u32 | reserved，必须为 0 |

响应头固定 40 字节，后接变长 payload：

| 偏移 | 类型 | 字段 |
| --- | --- | --- |
| 0 | u32 | magic `R3AI` |
| 4 | u16 | version `1` |
| 6 | u16 | status：0 OK、1 Invalid、2 NotReady |
| 8 | u32 | sequence |
| 12 | u32 | payload 字节数 |
| 16 | u32 | plane generation |
| 20 | u32 | AI 实例数 |
| 24 | u64 | 导出 Unix 毫秒时间 |
| 32 | u32 | 因锁竞争跳过的采样数 |
| 36 | u32 | reserved，固定 0 |

Payload 是连续的实例记录。每条记录开头的 `u32 record_size` 包含自身及后续战略状态，可让未来版本追加字段。v1 固定前缀为 168 字节，每个战略状态再占 12 字节。CLI 会拒绝超过 64 KiB、数量越界、记录边界错误或 magic/version/sequence 不匹配的响应。


# AI Data Plane 与单次 Probe

当前版本在已校验的 `ra3_1.12.game` 内维护一个只读、固定容量的 AI data plane。它暂时只提供 CLI，不注册 MCP 工具。

本页只描述已实现的观测面。如何在不破坏 AI 状态机的前提下，通过游戏逻辑线程调用原生 setter、预算、战略选择、目标和战术生命周期入口，见 [AI 控制面研究](AI_CONTROL.md)。其中的写命令仍是设计，不属于当前协议 v3。

## 执行边界

- ASI 在 `SkirmishAI::Update`（VA `0x00909A90`，RVA `0x00509A90`）返回后采样。
- 游戏逻辑线程只读取已经确认的对象偏移，并把结果复制到进程内固定容量快照；不分配内存、不访问文件或管道。
- 发布使用 `try_lock`。CLI 导出线程恰好占用 plane 时，本帧采样直接跳过，并累计 `skipped_captures`，游戏线程不等待。
- 独立管道工作线程从 plane 复制稳定快照，再以二进制协议发送。JSON 编码和文件写入全部发生在外部 `ra3ctl.exe`。
- Plane 与引擎容量对齐为 20 个玩家槽。每个 AI 以经验证的 `Player+0x20` 槽位号索引。单实例上限为 64 个战略状态、16 个预算状态、96 个建造状态、32 个目标和 64 个活跃战术；超过上限时设置对应截断标志，原始总数仍保留。
- 身份链必须同时满足 `SkirmishAI+0x8A8 -> Player`、`Player+0x90 == SkirmishAI`，以及 `PlayerList+0x30+slot*4 == Player`。失败样本不进入 plane，并累计 `rejected_identities`。

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

## JSON 的两个区域

顶层除 `schema` 外只有两个数据区域：

- `rough_snapshot`：面向人和 LLM 的当前态。按玩家列出 Personality、指令、推断阶段、候选战略、已选目标、活跃战术、当前预算分配和建造候选。
- `raw_data`：可复核证据。包含目标版本、plane 元数据以及每个实例的地址、AssetId、原始 float bits、vector 边界/容量、银行对象字、目标记录和战术对象字。

`rough_snapshot.strategy.phase` 是由“是否有活跃战术、是否已有目标状态、StrategyManager 是否启用”推导的摘要，不是游戏原生枚举。`ready_or_waiting` 同样只表示目标记录的可见标志满足一部分就绪条件；无法在无副作用的快照中复现的内部检查仍留在 `raw_data.status_bits`。

## 可翻译字段与边界

| 数据 | rough 解析 | raw 保留 | 置信度 |
| --- | --- | --- | --- |
| Player | `player_id`、active/defeated/disabled、Skirmish AI controller | lookup key、control type、地址和状态原值 | 已验证双向 PlayerList 索引 |
| Personality | `1SovietBalanced` 等 ID、PersonalityType、UI key | AssetId 指针、type/instance ID、definition 地址 | 官方 XML ID + 游戏 AssetId 哈希 |
| strategic directive | Invalid/Normal/Assault/HoldPosition/HoldObject/Help | i32 原值 | 官方枚举 |
| 战略/预算/建造状态 | 官方状态名、score、eligible、排序后的候选 | wrapper/definition/AssetId、float bits、flag、vector 元数据 | 官方 XML ID + 已逆 wrapper 链 |
| 银行 | INVESTMENT、SPECIAL_OPERATIONS、PRODUCTION、DEFENSE、TECHNOLOGY、SLUSH_FUND 及当前 share | 六个 36 字节账户对象的 9 个 dword | 官方枚举 + 已逆账户布局 |
| 当前预算状态 | 由 Bank 当前 budget 指针回配预算状态 | 当前 budget 指针 | 指针等值验证；找不到时为 null |
| 战略目标 | object ID、所选战略状态、assigned/pending 摘要 | 0x2CC 记录地址、三个状态字节、选择帧和指针 | 状态/对象字段已验证；状态字节只原样命名 |
| 活跃战术 | SimpleAttack、FlankAttack、Defense、Expansion、Hunt 等类 | instance、vtable/RVA、目标/AI 指针、前 12 个 dword | 精确 vtable RVA 映射；未知类不猜测 |

尚未翻译为 rough 结论的内容包括：目标对象 ID 对应的具体单位/建筑名称、目标的完整路径与距离判定、战术内部队伍成员、建造队列中的具体对象、未命名状态位。它们需要继续补对象表或容器布局；当前版本不会用猜测值填充。

## 名称索引

`controller/src/ai_index.rs` 是生成文件，当前包含 35 个 Personality、184 个战略状态、6 个预算状态和 70 个建造状态。来源是 Electronic Arts 的 `CnC_Modding_Support/Red Alert 3/Xml/SkirmishAI`，仓库只保存标识符、显示 key 与计算后的 instance ID，不复制游戏 XML。

需要从新的官方 XML checkout 重建时执行：

```powershell
python .\scripts\Generate-AIIndex.py `
  'C:\path\to\CnC_Modding_Support\Red Alert 3\Xml\SkirmishAI' `
  .\controller\src\ai_index.rs
```

生成器实现游戏 `sub_418040` 使用的 ASCII 大小写不敏感 AssetId instance hash；索引按哈希排序并有已知 Personality/状态回归测试。

## raw_data 的主要字段

每份 JSON 同时携带导出时间、plane generation、跳过/拒绝采样数，以及每个 AI 实例自己的采集时间和游戏帧。每个实例包括：

- `player_id`：当前 PlayerList 生命周期内稳定的 0..19 槽位号；JSON 同时标记 `kind=player_list_slot` 和验证状态。
- PlayerList 地址、活动数量原值、控制类型原值、查找键原值、current/local、defeated、`+0x106` 原始状态与派生活动资格。
- `SkirmishAI`、Player、ArmyDefinition、PersonalityRef/Definition 地址。
- Bank、BuildManager、BuildStateManager、StrategicStateManager、StrategyManager、EconomyManager 地址。
- disabled、last update frame、timer、strategic directive。
- StrategyManager enabled、interval、next frame。
- Personality update interval，以及 compatible factions、weighted structures、strategic/budget/build definition 列表的数量与地址。
- 战略、预算和建造状态 vector 的 begin/end/capacity、总数、容量及各状态的完整 AssetId 链。
- Bank 的当前 budget 指针与六个账户对象的原始字。
- StrategyManager 的目标记录数组和 TacticManager 的活跃 tactic 指针数组。

地址只用于本轮探针，不应跨进程或跨局缓存；稳定索引使用 `asset_instance_id`。

## AI 快照协议 v3

所有整数均为小端。请求固定 16 字节：

| 偏移 | 类型 | 字段 |
| --- | --- | --- |
| 0 | u32 | magic `0x49413352`，ASCII `R3AI` |
| 4 | u16 | version `3` |
| 6 | u16 | operation `0`：读取最新 plane |
| 8 | u32 | sequence |
| 12 | u32 | reserved，必须为 0 |

响应头固定 48 字节，后接变长 payload：

| 偏移 | 类型 | 字段 |
| --- | --- | --- |
| 0 | u32 | magic `R3AI` |
| 4 | u16 | version `3` |
| 6 | u16 | status：0 OK、1 Invalid、2 NotReady |
| 8 | u32 | sequence |
| 12 | u32 | payload 字节数 |
| 16 | u32 | plane generation |
| 20 | u32 | AI 实例数 |
| 24 | u64 | 导出 Unix 毫秒时间 |
| 32 | u32 | 因锁竞争跳过的采样数 |
| 36 | u32 | agent player-slot bitmask，低 20 位有效 |
| 40 | u32 | 因玩家身份校验失败而拒绝的采样数 |
| 44 | u32 | reserved，固定 0 |

Payload 按 player slot 升序包含连续实例记录。每条记录开头的 `u32 record_size` 包含自身、540 字节固定前缀及后续变长区：

| 变长项 | 单项大小 | 最大项数 |
| --- | ---: | ---: |
| strategic state | 28 字节 | 64 |
| budget state | 28 字节 | 16 |
| build state | 28 字节 | 96 |
| target record 摘要 | 40 字节 | 32 |
| tactic record 摘要 | 56 字节 | 64 |

固定前缀包含所有旧 v2 字段，并新增 image base、Personality AssetId、三组状态 vector 元数据、六个银行账户原始字，以及 target/tactic manager 元数据。CLI 会校验 agent 数量、slot bitmask、记录内 slot、重复槽、各区计数上限、精确记录大小、magic/version/sequence 和 512 KiB payload 上限。



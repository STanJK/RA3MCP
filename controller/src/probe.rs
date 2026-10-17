use crate::json::Json;
use crate::pipe::Pipe;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const MAGIC: u32 = 0x4941_3352; // bytes: R3AI
const VERSION: u16 = 2;
const REQUEST_SIZE: usize = 16;
const RESPONSE_HEADER_SIZE: usize = 48;
const AGENT_PREFIX_SIZE: usize = 212;
const MAX_AGENTS: u32 = 20;
const MAX_STATES: u32 = 64;
const MAX_PAYLOAD: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct StrategicStateDump {
    pub definition: u32,
    pub score: f32,
    pub score_bits: u32,
    pub flags: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentDump {
    pub flags: u32,
    pub capture_sequence: u32,
    pub game_frame: u32,
    pub captured_unix_ms: u64,
    pub skirmish_ai: u32,
    pub bank: u32,
    pub build_manager: u32,
    pub build_state_manager: u32,
    pub strategic_state_manager: u32,
    pub strategy_manager: u32,
    pub economy_manager: u32,
    pub player: u32,
    pub army_definition: u32,
    pub personality_ref: u32,
    pub personality_definition: u32,
    pub player_slot: u32,
    pub player_list: u32,
    pub player_lookup_key: u32,
    pub player_control_type: u32,
    pub player_ai: u32,
    pub player_skirmish_ai: u32,
    pub player_list_active_count: u32,
    pub player_raw_flag_106: u32,
    pub player_defeated: bool,
    pub player_is_current_or_local: bool,
    pub player_active_eligible: bool,
    pub disabled: bool,
    pub last_update_frame: u32,
    pub timer: i32,
    pub strategic_directive: i32,
    pub state_next_evaluation_frame: u32,
    pub strategy_enabled: bool,
    pub strategy_interval: u32,
    pub strategy_next_frame: u32,
    pub personality_update_interval: f32,
    pub personality_update_interval_bits: u32,
    pub compatible_faction_count: u32,
    pub compatible_factions: u32,
    pub weighted_structure_count: u32,
    pub weighted_structures: u32,
    pub strategic_definition_count: u32,
    pub strategic_definitions: u32,
    pub budget_definition_count: u32,
    pub budget_definitions: u32,
    pub build_definition_count: u32,
    pub build_definitions: u32,
    pub state_begin: u32,
    pub state_end: u32,
    pub state_capacity_end: u32,
    pub state_total_count: u32,
    pub state_count: u32,
    pub state_capacity_count: u32,
    pub states: Vec<StrategicStateDump>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlaneDump {
    pub pid: u32,
    pub generation: u32,
    pub skipped_captures: u32,
    pub agent_slot_mask: u32,
    pub rejected_identities: u32,
    pub exported_unix_ms: u64,
    pub agents: Vec<AgentDump>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
fn u16_at(bytes: &[u8], at: usize) -> io::Result<u16> {
    Ok(u16::from_le_bytes(bytes.get(at..at + 2).ok_or_else(|| invalid("truncated AI data-plane field"))?.try_into().unwrap()))
}
fn u32_at(bytes: &[u8], at: usize) -> io::Result<u32> {
    Ok(u32::from_le_bytes(bytes.get(at..at + 4).ok_or_else(|| invalid("truncated AI data-plane field"))?.try_into().unwrap()))
}
fn u64_at(bytes: &[u8], at: usize) -> io::Result<u64> {
    Ok(u64::from_le_bytes(bytes.get(at..at + 8).ok_or_else(|| invalid("truncated AI data-plane field"))?.try_into().unwrap()))
}

struct Reader<'a> { bytes: &'a [u8], at: usize }
impl Reader<'_> {
    fn u32(&mut self) -> io::Result<u32> { let value = u32_at(self.bytes, self.at)?; self.at += 4; Ok(value) }
    fn i32(&mut self) -> io::Result<i32> { Ok(self.u32()? as i32) }
    fn u64(&mut self) -> io::Result<u64> { let value = u64_at(self.bytes, self.at)?; self.at += 8; Ok(value) }
}

fn parse_agent(record: &[u8]) -> io::Result<AgentDump> {
    if record.len() < AGENT_PREFIX_SIZE { return Err(invalid("AI agent record is shorter than v2 prefix")); }
    let declared = u32_at(record, 0)? as usize;
    if declared != record.len() { return Err(invalid("AI agent record size mismatch")); }
    let mut r = Reader { bytes: record, at: 4 };
    let flags = r.u32()?;
    let capture_sequence = r.u32()?;
    let game_frame = r.u32()?;
    let captured_unix_ms = r.u64()?;
    let skirmish_ai = r.u32()?;
    let bank = r.u32()?;
    let build_manager = r.u32()?;
    let build_state_manager = r.u32()?;
    let strategic_state_manager = r.u32()?;
    let strategy_manager = r.u32()?;
    let economy_manager = r.u32()?;
    let player = r.u32()?;
    let army_definition = r.u32()?;
    let personality_ref = r.u32()?;
    let personality_definition = r.u32()?;
    let player_slot = r.u32()?;
    let player_list = r.u32()?;
    let player_lookup_key = r.u32()?;
    let player_control_type = r.u32()?;
    let player_ai = r.u32()?;
    let player_skirmish_ai = r.u32()?;
    let player_list_active_count = r.u32()?;
    let player_raw_flag_106 = r.u32()?;
    let player_defeated = r.u32()? != 0;
    let player_is_current_or_local = r.u32()? != 0;
    let player_active_eligible = r.u32()? != 0;
    let disabled = r.u32()? != 0;
    let last_update_frame = r.u32()?;
    let timer = r.i32()?;
    let strategic_directive = r.i32()?;
    let state_next_evaluation_frame = r.u32()?;
    let strategy_enabled = r.u32()? != 0;
    let strategy_interval = r.u32()?;
    let strategy_next_frame = r.u32()?;
    let personality_update_interval_bits = r.u32()?;
    let compatible_faction_count = r.u32()?;
    let compatible_factions = r.u32()?;
    let weighted_structure_count = r.u32()?;
    let weighted_structures = r.u32()?;
    let strategic_definition_count = r.u32()?;
    let strategic_definitions = r.u32()?;
    let budget_definition_count = r.u32()?;
    let budget_definitions = r.u32()?;
    let build_definition_count = r.u32()?;
    let build_definitions = r.u32()?;
    let state_begin = r.u32()?;
    let state_end = r.u32()?;
    let state_capacity_end = r.u32()?;
    let state_total_count = r.u32()?;
    let state_count = r.u32()?;
    let state_capacity_count = r.u32()?;
    if r.at != AGENT_PREFIX_SIZE || state_count > MAX_STATES || state_count > state_total_count {
        return Err(invalid("invalid AI strategic-state counts"));
    }
    let expected = AGENT_PREFIX_SIZE.checked_add(state_count as usize * 12)
        .ok_or_else(|| invalid("AI agent record size overflow"))?;
    if expected != record.len() { return Err(invalid("AI strategic-state payload size mismatch")); }
    let mut states = Vec::with_capacity(state_count as usize);
    for _ in 0..state_count {
        let definition = r.u32()?;
        let score_bits = r.u32()?;
        states.push(StrategicStateDump { definition, score: f32::from_bits(score_bits), score_bits, flags: r.u32()? });
    }
    Ok(AgentDump {
        flags, capture_sequence, game_frame, captured_unix_ms, skirmish_ai, bank, build_manager,
        build_state_manager, strategic_state_manager, strategy_manager, economy_manager, player,
        army_definition, personality_ref, personality_definition, player_slot, player_list,
        player_lookup_key, player_control_type, player_ai, player_skirmish_ai,
        player_list_active_count, player_raw_flag_106, player_defeated,
        player_is_current_or_local, player_active_eligible, disabled, last_update_frame, timer,
        strategic_directive, state_next_evaluation_frame, strategy_enabled, strategy_interval,
        strategy_next_frame, personality_update_interval: f32::from_bits(personality_update_interval_bits),
        personality_update_interval_bits, compatible_faction_count, compatible_factions,
        weighted_structure_count, weighted_structures, strategic_definition_count, strategic_definitions,
        budget_definition_count, budget_definitions, build_definition_count, build_definitions,
        state_begin, state_end, state_capacity_end, state_total_count, state_count, state_capacity_count, states,
    })
}

fn parse_response(pid: u32, sequence: u32, header: &[u8; RESPONSE_HEADER_SIZE], payload: &[u8]) -> io::Result<PlaneDump> {
    if u32_at(header, 0)? != MAGIC || u16_at(header, 4)? != VERSION || u32_at(header, 8)? != sequence {
        return Err(invalid("AI data-plane magic, version, or sequence mismatch"));
    }
    if u32_at(header, 12)? as usize != payload.len() || u32_at(header, 44)? != 0 {
        return Err(invalid("malformed AI data-plane response header"));
    }
    match u16_at(header, 6)? {
        0 => {}
        1 => return Err(invalid("AI bridge rejected the probe request")),
        2 => return Err(io::Error::new(io::ErrorKind::WouldBlock,
            "AI data plane is ready but has not observed a SkirmishAI update yet; enter an active skirmish and retry")),
        status => return Err(invalid(format!("unknown AI data-plane status {status}"))),
    }
    let generation = u32_at(header, 16)?;
    let agent_count = u32_at(header, 20)?;
    let exported_unix_ms = u64_at(header, 24)?;
    let skipped_captures = u32_at(header, 32)?;
    let agent_slot_mask = u32_at(header, 36)?;
    let rejected_identities = u32_at(header, 40)?;
    if agent_count > MAX_AGENTS { return Err(invalid("AI data plane returned too many agents")); }
    let valid_mask = (1u32 << MAX_AGENTS) - 1;
    if agent_slot_mask & !valid_mask != 0 || agent_slot_mask.count_ones() != agent_count {
        return Err(invalid("AI agent count and player-slot mask disagree"));
    }
    let mut agents = Vec::with_capacity(agent_count as usize);
    let mut parsed_slot_mask = 0u32;
    let mut at = 0usize;
    for _ in 0..agent_count {
        let size = u32_at(payload, at)? as usize;
        if size < AGENT_PREFIX_SIZE || at.checked_add(size).filter(|end| *end <= payload.len()).is_none() {
            return Err(invalid("invalid AI agent record boundary"));
        }
        let agent = parse_agent(&payload[at..at + size])?;
        if agent.player_slot >= MAX_AGENTS || agent_slot_mask & (1u32 << agent.player_slot) == 0 ||
           parsed_slot_mask & (1u32 << agent.player_slot) != 0 {
            return Err(invalid("AI agent has an invalid or duplicate player slot"));
        }
        parsed_slot_mask |= 1u32 << agent.player_slot;
        agents.push(agent);
        at += size;
    }
    if at != payload.len() { return Err(invalid("trailing bytes in AI data-plane payload")); }
    if parsed_slot_mask != agent_slot_mask {
        return Err(invalid("AI payload does not cover the advertised player slots"));
    }
    Ok(PlaneDump { pid, generation, skipped_captures, agent_slot_mask,
        rejected_identities, exported_unix_ms, agents })
}

pub fn capture(pid: u32) -> io::Result<PlaneDump> {
    let cancel = AtomicBool::new(false);
    let pipe = Pipe::connect_ai(pid, &cancel)?;
    let sequence = 1u32;
    let mut request = [0u8; REQUEST_SIZE];
    request[0..4].copy_from_slice(&MAGIC.to_le_bytes());
    request[4..6].copy_from_slice(&VERSION.to_le_bytes());
    request[6..8].copy_from_slice(&0u16.to_le_bytes());
    request[8..12].copy_from_slice(&sequence.to_le_bytes());
    pipe.write(&request, &cancel)?;
    let mut header = [0u8; RESPONSE_HEADER_SIZE];
    pipe.read(&mut header, &cancel)?;
    let payload_size = u32_at(&header, 12)? as usize;
    if payload_size > MAX_PAYLOAD { return Err(invalid("AI data-plane payload exceeds 64 KiB")); }
    let mut payload = vec![0u8; payload_size];
    pipe.read(&mut payload, &cancel)?;
    parse_response(pid, sequence, &header, &payload)
}

fn number(value: impl ToString) -> Json { Json::Number(value.to_string()) }
fn address(value: u32) -> Json { format!("0x{value:08X}").into() }
fn float(value: f32) -> Json {
    if value.is_finite() { Json::Number(format!("{value:.9}")) } else { Json::Null }
}
fn flags(value: u32) -> Json {
    Json::object([
        ("raw", number(value)),
        ("self_readable", Json::Bool(value & (1 << 0) != 0)),
        ("game_clock_readable", Json::Bool(value & (1 << 1) != 0)),
        ("state_vector_valid", Json::Bool(value & (1 << 2) != 0)),
        ("state_vector_truncated", Json::Bool(value & (1 << 3) != 0)),
        ("personality_readable", Json::Bool(value & (1 << 4) != 0)),
        ("strategy_manager_readable", Json::Bool(value & (1 << 5) != 0)),
        ("player_readable", Json::Bool(value & (1 << 6) != 0)),
        ("player_owner_verified", Json::Bool(value & (1 << 7) != 0)),
        ("player_list_readable", Json::Bool(value & (1 << 8) != 0)),
        ("player_slot_verified", Json::Bool(value & (1 << 9) != 0)),
    ])
}

fn utc(unix_ms: u64) -> String {
    let total_seconds = unix_ms / 1000;
    let milliseconds = unix_ms % 1000;
    let days = (total_seconds / 86400) as i64;
    let seconds = total_seconds % 86400;
    let hour = seconds / 3600;
    let minute = seconds % 3600 / 60;
    let second = seconds % 60;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let day_of_era = z - era * 146097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{milliseconds:03}Z")
}

impl PlaneDump {
    pub fn json(&self) -> String {
        let agents = self.agents.iter().map(|agent| {
            let states = agent.states.iter().enumerate().map(|(index, state)| Json::object([
                ("index", number(index)),
                ("definition", address(state.definition)),
                ("score", float(state.score)),
                ("score_bits", format!("0x{:08X}", state.score_bits).into()),
                ("flags", number(state.flags)),
            ])).collect();
            Json::object([
                ("player_id", number(agent.player_slot)),
                ("player_identity", Json::object([
                    ("kind", "player_list_slot".into()),
                    ("slot", number(agent.player_slot)),
                    ("verified", Json::Bool(agent.flags & (1 << 9) != 0)),
                    ("lookup_key_raw", format!("0x{:08X}", agent.player_lookup_key).into()),
                    ("control_type_raw", number(agent.player_control_type)),
                    ("observed_ai_path", Json::Bool(agent.player_control_type == 1)),
                    ("active_count_raw", number(agent.player_list_active_count)),
                    ("raw_flag_106", number(agent.player_raw_flag_106)),
                    ("defeated", agent.player_defeated.into()),
                    ("active_eligible", agent.player_active_eligible.into()),
                    ("is_current_or_local", agent.player_is_current_or_local.into()),
                ])),
                ("addresses", Json::object([
                    ("skirmish_ai", address(agent.skirmish_ai)), ("bank", address(agent.bank)),
                    ("build_manager", address(agent.build_manager)),
                    ("build_state_manager", address(agent.build_state_manager)),
                    ("strategic_state_manager", address(agent.strategic_state_manager)),
                    ("strategy_manager", address(agent.strategy_manager)),
                    ("economy_manager", address(agent.economy_manager)), ("player", address(agent.player)),
                    ("army_definition", address(agent.army_definition)),
                    ("personality_ref", address(agent.personality_ref)),
                    ("personality_definition", address(agent.personality_definition)),
                    ("player_list", address(agent.player_list)),
                    ("player_ai", address(agent.player_ai)),
                    ("player_skirmish_ai", address(agent.player_skirmish_ai)),
                ])),
                ("capture_sequence", number(agent.capture_sequence)),
                ("captured_unix_ms", number(agent.captured_unix_ms)),
                ("captured_utc", utc(agent.captured_unix_ms).into()),
                ("flags", flags(agent.flags)),
                ("game_frame", number(agent.game_frame)),
                ("runtime", Json::object([
                    ("disabled", agent.disabled.into()), ("last_update_frame", number(agent.last_update_frame)),
                    ("timer", number(agent.timer)), ("strategic_directive", number(agent.strategic_directive)),
                ])),
                ("strategy", Json::object([
                    ("enabled", agent.strategy_enabled.into()), ("interval", number(agent.strategy_interval)),
                    ("next_frame", number(agent.strategy_next_frame)),
                ])),
                ("personality", Json::object([
                    ("update_interval", float(agent.personality_update_interval)),
                    ("update_interval_bits", format!("0x{:08X}", agent.personality_update_interval_bits).into()),
                    ("compatible_factions", Json::object([("count", number(agent.compatible_faction_count)),
                        ("address", address(agent.compatible_factions))])),
                    ("weighted_structures", Json::object([("count", number(agent.weighted_structure_count)),
                        ("address", address(agent.weighted_structures))])),
                    ("strategic_definitions", Json::object([("count", number(agent.strategic_definition_count)),
                        ("address", address(agent.strategic_definitions))])),
                    ("budget_definitions", Json::object([("count", number(agent.budget_definition_count)),
                        ("address", address(agent.budget_definitions))])),
                    ("build_definitions", Json::object([("count", number(agent.build_definition_count)),
                        ("address", address(agent.build_definitions))])),
                ])),
                ("strategic_states", Json::object([
                    ("next_evaluation_frame", number(agent.state_next_evaluation_frame)),
                    ("vector_begin", address(agent.state_begin)), ("vector_end", address(agent.state_end)),
                    ("vector_capacity_end", address(agent.state_capacity_end)),
                    ("total_count", number(agent.state_total_count)), ("exported_count", number(agent.state_count)),
                    ("capacity_count", number(agent.state_capacity_count)), ("items", Json::Array(states)),
                ])),
            ])
        }).collect();
        let root = Json::object([
            ("schema", "ra3mcp.ai-plane.v2".into()),
            ("target", Json::object([
                ("executable", "ra3_1.12.game".into()),
                ("sha256", "dc54c54aa40d48bf2605cd5616f01e19fb1b3859b9dbe03dfa6dc28e6921e4b0".into()),
                ("pid", number(self.pid)),
            ])),
            ("plane_generation", number(self.generation)),
            ("skipped_captures", number(self.skipped_captures)),
            ("rejected_identities", number(self.rejected_identities)),
            ("agent_slot_mask", format!("0x{:08X}", self.agent_slot_mask).into()),
            ("exported_unix_ms", number(self.exported_unix_ms)),
            ("exported_utc", utc(self.exported_unix_ms).into()),
            ("agents", Json::Array(agents)),
        ]);
        root.encode_pretty() + "\n"
    }

    pub fn write(&self, output: Option<&Path>) -> io::Result<PathBuf> {
        let path = output.map(Path::to_path_buf).unwrap_or_else(|| {
            let stamp = utc(self.exported_unix_ms).replace('-', "").replace(':', "");
            PathBuf::from("dumps").join(format!("ra3-ai-probe-{}-{stamp}-g{}.json", self.pid, self.generation))
        });
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) { fs::create_dir_all(parent)?; }
        if path.exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists,
                format!("refusing to overwrite existing probe dump {}", path.display())));
        }
        let temporary = path.with_extension(format!("json.tmp-{}", std::process::id()));
        let result = (|| {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
            file.write_all(self.json().as_bytes())?;
            file.flush()?;
            fs::rename(&temporary, &path)?;
            Ok(path.clone())
        })();
        if result.is_err() { let _ = fs::remove_file(&temporary); }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response() -> ([u8; RESPONSE_HEADER_SIZE], Vec<u8>) {
        let mut record = vec![0u8; AGENT_PREFIX_SIZE + 12];
        let length = record.len() as u32;
        record[0..4].copy_from_slice(&length.to_le_bytes());
        record[8..12].copy_from_slice(&9u32.to_le_bytes());
        record[12..16].copy_from_slice(&42u32.to_le_bytes());
        record[16..24].copy_from_slice(&1234u64.to_le_bytes());
        record[24..28].copy_from_slice(&0x1234_0000u32.to_le_bytes());
        record[52..56].copy_from_slice(&0x2345_0000u32.to_le_bytes());
        record[68..72].copy_from_slice(&3u32.to_le_bytes());
        record[200..204].copy_from_slice(&1u32.to_le_bytes());
        record[204..208].copy_from_slice(&1u32.to_le_bytes());
        record[208..212].copy_from_slice(&1u32.to_le_bytes());
        record[212..216].copy_from_slice(&0x7654_0000u32.to_le_bytes());
        record[216..220].copy_from_slice(&0.5f32.to_bits().to_le_bytes());
        record[220..224].copy_from_slice(&1u32.to_le_bytes());
        let mut header = [0u8; RESPONSE_HEADER_SIZE];
        header[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        header[4..6].copy_from_slice(&VERSION.to_le_bytes());
        header[8..12].copy_from_slice(&1u32.to_le_bytes());
        header[12..16].copy_from_slice(&(record.len() as u32).to_le_bytes());
        header[16..20].copy_from_slice(&7u32.to_le_bytes());
        header[20..24].copy_from_slice(&1u32.to_le_bytes());
        header[24..32].copy_from_slice(&1234u64.to_le_bytes());
        header[36..40].copy_from_slice(&(1u32 << 3).to_le_bytes());
        header[40..44].copy_from_slice(&2u32.to_le_bytes());
        (header, record)
    }

    #[test]
    fn parses_v2_player_identity_and_emits_timestamped_json() {
        let (header, payload) = response();
        let dump = parse_response(88, 1, &header, &payload).unwrap();
        assert_eq!(dump.generation, 7);
        assert_eq!(dump.agent_slot_mask, 1 << 3);
        assert_eq!(dump.rejected_identities, 2);
        assert_eq!(dump.agents[0].player_slot, 3);
        assert_eq!(dump.agents[0].states[0].score, 0.5);
        let json = dump.json();
        assert!(json.contains("ra3mcp.ai-plane.v2"));
        assert!(json.contains("\"player_id\": 3"));
        assert!(json.contains("0x76540000"));
        assert_eq!(utc(0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn rejects_truncation_and_oversized_counts() {
        let (header, mut payload) = response();
        payload[204..208].copy_from_slice(&65u32.to_le_bytes());
        assert!(parse_response(88, 1, &header, &payload).is_err());
        assert!(parse_response(88, 2, &header, &payload).is_err());
    }
}


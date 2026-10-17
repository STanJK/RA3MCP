use crate::ai_index;
use crate::json::Json;
use crate::pipe::Pipe;
use std::cmp::Ordering;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const MAGIC: u32 = 0x4941_3352; // bytes: R3AI
const VERSION: u16 = 3;
const REQUEST_SIZE: usize = 16;
const RESPONSE_HEADER_SIZE: usize = 48;
const AGENT_PREFIX_SIZE: usize = 540;
const MAX_AGENTS: u32 = 20;
const MAX_STRATEGIC_STATES: u32 = 64;
const MAX_BUDGET_STATES: u32 = 16;
const MAX_BUILD_STATES: u32 = 96;
const MAX_TARGETS: u32 = 32;
const MAX_TACTICS: u32 = 64;
const BANK_ACCOUNT_COUNT: usize = 6;
const BANK_ACCOUNT_WORDS: usize = 9;
const TACTIC_WORDS: usize = 12;
const STATE_ENTRY_SIZE: usize = 28;
const TARGET_ENTRY_SIZE: usize = 40;
const TACTIC_ENTRY_SIZE: usize = 56;
const MAX_PAYLOAD: usize = 512 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct StateDump {
    pub wrapper: u32,
    pub score: f32,
    pub score_bits: u32,
    pub flags: u32,
    pub definition: u32,
    pub asset_id: u32,
    pub asset_type_id: u32,
    pub asset_instance_id: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BankAccountDump {
    pub words: [u32; BANK_ACCOUNT_WORDS],
}

#[derive(Debug, Clone, PartialEq)]
pub struct TargetDump {
    pub record: u32,
    pub state_wrapper: u32,
    pub tactic: u32,
    pub object_id: u32,
    pub status_bits: u32,
    pub state_selection_frame: u32,
    pub state_definition: u32,
    pub state_asset_id: u32,
    pub state_asset_type_id: u32,
    pub state_asset_instance_id: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TacticDump {
    pub instance: u32,
    pub words: [u32; TACTIC_WORDS],
    pub vtable_rva: u32,
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
    pub image_base: u32,
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
    pub personality_asset_id: u32,
    pub personality_asset_type_id: u32,
    pub personality_asset_instance_id: u32,
    pub state_begin: u32,
    pub state_end: u32,
    pub state_capacity_end: u32,
    pub state_total_count: u32,
    pub state_count: u32,
    pub state_capacity_count: u32,
    pub budget_state_begin: u32,
    pub budget_state_end: u32,
    pub budget_state_capacity_end: u32,
    pub budget_state_total_count: u32,
    pub budget_state_count: u32,
    pub budget_state_capacity_count: u32,
    pub build_state_begin: u32,
    pub build_state_end: u32,
    pub build_state_capacity_end: u32,
    pub build_state_total_count: u32,
    pub build_state_count: u32,
    pub build_state_capacity_count: u32,
    pub current_budget: u32,
    pub bank_accounts: Vec<BankAccountDump>,
    pub target_manager: u32,
    pub target_begin: u32,
    pub target_total_count: u32,
    pub target_count: u32,
    pub tactic_manager: u32,
    pub tactic_begin: u32,
    pub tactic_end: u32,
    pub tactic_capacity_end: u32,
    pub tactic_total_count: u32,
    pub tactic_count: u32,
    pub tactic_capacity_count: u32,
    pub states: Vec<StateDump>,
    pub budget_states: Vec<StateDump>,
    pub build_states: Vec<StateDump>,
    pub targets: Vec<TargetDump>,
    pub tactics: Vec<TacticDump>,
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

fn parse_states(r: &mut Reader<'_>, count: u32) -> io::Result<Vec<StateDump>> {
    let mut states = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let wrapper = r.u32()?;
        let score_bits = r.u32()?;
        states.push(StateDump {
            wrapper,
            score: f32::from_bits(score_bits),
            score_bits,
            flags: r.u32()?,
            definition: r.u32()?,
            asset_id: r.u32()?,
            asset_type_id: r.u32()?,
            asset_instance_id: r.u32()?,
        });
    }
    Ok(states)
}

fn parse_agent(record: &[u8]) -> io::Result<AgentDump> {
    if record.len() < AGENT_PREFIX_SIZE { return Err(invalid("AI agent record is shorter than v3 prefix")); }
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
    let image_base = r.u32()?;
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
    let personality_asset_id = r.u32()?;
    let personality_asset_type_id = r.u32()?;
    let personality_asset_instance_id = r.u32()?;
    let state_begin = r.u32()?;
    let state_end = r.u32()?;
    let state_capacity_end = r.u32()?;
    let state_total_count = r.u32()?;
    let state_count = r.u32()?;
    let state_capacity_count = r.u32()?;
    let budget_state_begin = r.u32()?;
    let budget_state_end = r.u32()?;
    let budget_state_capacity_end = r.u32()?;
    let budget_state_total_count = r.u32()?;
    let budget_state_count = r.u32()?;
    let budget_state_capacity_count = r.u32()?;
    let build_state_begin = r.u32()?;
    let build_state_end = r.u32()?;
    let build_state_capacity_end = r.u32()?;
    let build_state_total_count = r.u32()?;
    let build_state_count = r.u32()?;
    let build_state_capacity_count = r.u32()?;
    let current_budget = r.u32()?;
    let mut bank_accounts = Vec::with_capacity(BANK_ACCOUNT_COUNT);
    for _ in 0..BANK_ACCOUNT_COUNT {
        let mut words = [0u32; BANK_ACCOUNT_WORDS];
        for word in &mut words { *word = r.u32()?; }
        bank_accounts.push(BankAccountDump { words });
    }
    let target_manager = r.u32()?;
    let target_begin = r.u32()?;
    let target_total_count = r.u32()?;
    let target_count = r.u32()?;
    let tactic_manager = r.u32()?;
    let tactic_begin = r.u32()?;
    let tactic_end = r.u32()?;
    let tactic_capacity_end = r.u32()?;
    let tactic_total_count = r.u32()?;
    let tactic_count = r.u32()?;
    let tactic_capacity_count = r.u32()?;
    if r.at != AGENT_PREFIX_SIZE || state_count > MAX_STRATEGIC_STATES ||
       budget_state_count > MAX_BUDGET_STATES || build_state_count > MAX_BUILD_STATES ||
       target_count > MAX_TARGETS || tactic_count > MAX_TACTICS ||
       state_count > state_total_count || budget_state_count > budget_state_total_count ||
       build_state_count > build_state_total_count || target_count > target_total_count ||
       tactic_count > tactic_total_count {
        return Err(invalid("invalid AI data-plane section counts"));
    }
    let expected = AGENT_PREFIX_SIZE
        .checked_add(state_count as usize * STATE_ENTRY_SIZE).and_then(|n| n.checked_add(budget_state_count as usize * STATE_ENTRY_SIZE))
        .and_then(|n| n.checked_add(build_state_count as usize * STATE_ENTRY_SIZE))
        .and_then(|n| n.checked_add(target_count as usize * TARGET_ENTRY_SIZE))
        .and_then(|n| n.checked_add(tactic_count as usize * TACTIC_ENTRY_SIZE))
        .ok_or_else(|| invalid("AI agent record size overflow"))?;
    if expected != record.len() { return Err(invalid("AI agent variable-section size mismatch")); }
    let states = parse_states(&mut r, state_count)?;
    let budget_states = parse_states(&mut r, budget_state_count)?;
    let build_states = parse_states(&mut r, build_state_count)?;
    let mut targets = Vec::with_capacity(target_count as usize);
    for _ in 0..target_count {
        targets.push(TargetDump {
            record: r.u32()?, state_wrapper: r.u32()?, tactic: r.u32()?, object_id: r.u32()?,
            status_bits: r.u32()?, state_selection_frame: r.u32()?, state_definition: r.u32()?,
            state_asset_id: r.u32()?, state_asset_type_id: r.u32()?, state_asset_instance_id: r.u32()?,
        });
    }
    let mut tactics = Vec::with_capacity(tactic_count as usize);
    for _ in 0..tactic_count {
        let instance = r.u32()?;
        let mut words = [0u32; TACTIC_WORDS];
        for word in &mut words { *word = r.u32()?; }
        tactics.push(TacticDump { instance, words, vtable_rva: r.u32()? });
    }
    if r.at != record.len() { return Err(invalid("trailing bytes in AI agent record")); }
    Ok(AgentDump {
        flags, capture_sequence, game_frame, captured_unix_ms, skirmish_ai, bank, build_manager,
        build_state_manager, strategic_state_manager, strategy_manager, economy_manager, player,
        army_definition, personality_ref, personality_definition, image_base, player_slot, player_list,
        player_lookup_key, player_control_type, player_ai, player_skirmish_ai, player_list_active_count,
        player_raw_flag_106, player_defeated, player_is_current_or_local, player_active_eligible,
        disabled, last_update_frame, timer, strategic_directive, state_next_evaluation_frame,
        strategy_enabled, strategy_interval, strategy_next_frame,
        personality_update_interval: f32::from_bits(personality_update_interval_bits),
        personality_update_interval_bits, compatible_faction_count, compatible_factions,
        weighted_structure_count, weighted_structures, strategic_definition_count,
        strategic_definitions, budget_definition_count, budget_definitions, build_definition_count,
        build_definitions, personality_asset_id, personality_asset_type_id,
        personality_asset_instance_id, state_begin, state_end, state_capacity_end, state_total_count,
        state_count, state_capacity_count, budget_state_begin, budget_state_end,
        budget_state_capacity_end, budget_state_total_count, budget_state_count,
        budget_state_capacity_count, build_state_begin, build_state_end, build_state_capacity_end,
        build_state_total_count, build_state_count, build_state_capacity_count, current_budget,
        bank_accounts, target_manager, target_begin, target_total_count, target_count, tactic_manager,
        tactic_begin, tactic_end, tactic_capacity_end, tactic_total_count, tactic_count,
        tactic_capacity_count, states, budget_states, build_states, targets, tactics,
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
    if at != payload.len() || parsed_slot_mask != agent_slot_mask {
        return Err(invalid("AI payload does not match the advertised player slots"));
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
    if payload_size > MAX_PAYLOAD { return Err(invalid("AI data-plane payload exceeds 512 KiB")); }
    let mut payload = vec![0u8; payload_size];
    pipe.read(&mut payload, &cancel)?;
    parse_response(pid, sequence, &header, &payload)
}

fn number(value: impl ToString) -> Json { Json::Number(value.to_string()) }
fn address(value: u32) -> Json { format!("0x{value:08X}").into() }
fn hex32(value: u32) -> Json { format!("0x{value:08X}").into() }
fn float(value: f32) -> Json {
    if value.is_finite() { Json::Number(format!("{value:.9}")) } else { Json::Null }
}
fn optional_text(value: Option<&str>) -> Json { value.map_or(Json::Null, Into::into) }

fn flags(value: u32) -> Json {
    const NAMES: [&str; 20] = [
        "self_readable", "game_clock_readable", "strategic_vector_valid", "strategic_vector_truncated",
        "personality_readable", "strategy_manager_readable", "player_readable", "player_owner_verified",
        "player_list_readable", "player_slot_verified", "personality_identity_valid", "budget_vector_valid",
        "budget_vector_truncated", "build_vector_valid", "build_vector_truncated", "bank_readable",
        "target_manager_readable", "target_vector_truncated", "tactic_manager_readable", "tactic_vector_truncated",
    ];
    let mut fields = Vec::with_capacity(NAMES.len() + 1);
    fields.push(("raw", number(value)));
    fields.extend(NAMES.iter().enumerate().map(|(bit, name)| (*name, Json::Bool(value & (1 << bit) != 0))));
    Json::object(fields)
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

#[derive(Clone, Copy)]
enum StateKind { Strategic, Budget, Build }

fn state_name(kind: StateKind, instance_id: u32) -> Option<&'static str> {
    (match kind {
        StateKind::Strategic => ai_index::strategic_state(instance_id),
        StateKind::Budget => ai_index::budget_state(instance_id),
        StateKind::Build => ai_index::build_state(instance_id),
    }).map(|entry| entry.name)
}

fn rough_state(state: &StateDump, kind: StateKind) -> Json {
    Json::object([
        ("name", optional_text(state_name(kind, state.asset_instance_id))),
        ("asset_instance_id", hex32(state.asset_instance_id)),
        ("score", float(state.score)),
        ("eligible", Json::Bool(state.score.is_finite() && state.score > 0.0 && state.flags == 0)),
    ])
}

fn ranked_states(states: &[StateDump], kind: StateKind, limit: usize) -> Json {
    let mut candidates: Vec<&StateDump> = states.iter().filter(|state|
        state.score.is_finite() && state.score > 0.0 && state.flags == 0).collect();
    candidates.sort_by(|left, right| right.score.partial_cmp(&left.score).unwrap_or(Ordering::Equal));
    Json::Array(candidates.into_iter().take(limit).map(|state| rough_state(state, kind)).collect())
}

fn directive_name(value: i32) -> Option<&'static str> {
    match value { -1 => Some("Invalid"), 0 => Some("Normal"), 1 => Some("Assault"),
        2 => Some("HoldPosition"), 3 => Some("HoldObject"), 4 => Some("Help"), _ => None }
}

fn account_name(value: u32) -> Option<&'static str> {
    match value { 0 => Some("INVESTMENT"), 1 => Some("SPECIAL_OPERATIONS"), 2 => Some("PRODUCTION"),
        3 => Some("DEFENSE"), 4 => Some("TECHNOLOGY"), 5 => Some("SLUSH_FUND"), _ => None }
}

fn tactic_name(vtable_rva: u32) -> Option<(&'static str, &'static str)> {
    match vtable_rva {
        0x8679D4 => Some(("AIReactiveDefenseTactic", "ReactiveDefense")),
        0x867A0C => Some(("AISimpleDefenseTactic", "SimpleDefense")),
        0x867A44 => Some(("AIStaticDefenseTactic", "StaticDefense")),
        0x867A7C => Some(("AIExactExpansionTactic", "ExactExpansion")),
        0x867AB4 => Some(("AISimpleExpansionTactic", "SimpleExpansion")),
        0x867AEC => Some(("AIEngineerTactic", "Engineer")),
        0x867B38 => Some(("AIFlankAttackTactic", "FlankAttack")),
        0x867B74 => Some(("AIGarrisonTactic", "GarrisonBuilding")),
        0x867BC0 => Some(("AISimpleAttackTactic", "SimpleAttack")),
        0x867BFC => Some(("AISimpleSiegeTactic", "SimpleSiege")),
        0x867C38 => Some(("AIHuntTactic", "Hunt")),
        0x867C7C => Some(("AINullTactic", "NullTactic")),
        0x867CB4 => Some(("AIScriptedTactic", "Scripted")),
        _ => None,
    }
}

fn raw_state(state: &StateDump, index: usize) -> Json {
    Json::object([
        ("index", number(index)), ("wrapper", address(state.wrapper)),
        ("score", float(state.score)), ("score_bits", hex32(state.score_bits)),
        ("flags", number(state.flags)), ("definition", address(state.definition)),
        ("asset_id", address(state.asset_id)), ("asset_type_id", hex32(state.asset_type_id)),
        ("asset_instance_id", hex32(state.asset_instance_id)),
    ])
}

fn raw_state_vector(begin: u32, end: u32, capacity_end: u32, total: u32, capacity: u32,
                    states: &[StateDump]) -> Json {
    Json::object([
        ("vector_begin", address(begin)), ("vector_end", address(end)),
        ("vector_capacity_end", address(capacity_end)), ("total_count", number(total)),
        ("exported_count", number(states.len())), ("capacity_count", number(capacity)),
        ("items", Json::Array(states.iter().enumerate().map(|(index, state)| raw_state(state, index)).collect())),
    ])
}

fn rough_agent(agent: &AgentDump) -> Json {
    let personality = ai_index::personality(agent.personality_asset_instance_id);
    let current_budget = agent.budget_states.iter().find(|state|
        state.definition != 0 && state.definition.wrapping_add(12) == agent.current_budget);
    let accounts = agent.bank_accounts.iter().enumerate().map(|(index, account)| {
        let category = account.words[2];
        Json::object([
            ("index", number(index)), ("name", optional_text(account_name(category))),
            ("share", float(f32::from_bits(account.words[3]))),
        ])
    }).collect();
    let tactics = agent.tactics.iter().enumerate().map(|(index, tactic)| {
        let translated = tactic_name(tactic.vtable_rva);
        Json::object([
            ("index", number(index)), ("class", optional_text(translated.map(|value| value.0))),
            ("type", optional_text(translated.map(|value| value.1))),
            ("target_assigned", Json::Bool(tactic.words[10] != 0)),
        ])
    }).collect();
    let targets = agent.targets.iter().enumerate().map(|(index, target)| {
        let tactic = agent.tactics.iter().find(|tactic| tactic.instance == target.tactic);
        let translated = tactic.and_then(|tactic| tactic_name(tactic.vtable_rva));
        let assignment = if target.tactic != 0 { "assigned" }
            else if target.state_wrapper == 0 { "empty" }
            else if target.status_bits & 0x0001_0000 != 0 { "reselect_pending" }
            else if target.status_bits & 1 != 0 { "ready_or_waiting" }
            else { "pending" };
        Json::object([
            ("index", number(index)), ("object_id", number(target.object_id)),
            ("state", optional_text(state_name(StateKind::Strategic, target.state_asset_instance_id))),
            ("state_asset_instance_id", hex32(target.state_asset_instance_id)),
            ("assignment", assignment.into()),
            ("tactic", optional_text(translated.map(|value| value.1))),
        ])
    }).collect();
    let status = if agent.player_defeated { "defeated" } else if agent.disabled { "disabled" }
        else if agent.player_active_eligible { "active" } else { "inactive" };
    let phase = if !agent.tactics.is_empty() { "executing" } else if agent.targets.iter().any(|target| target.state_wrapper != 0) {
        "targeting"
    } else if agent.strategy_enabled { "planning" } else { "idle" };
    Json::object([
        ("player_id", number(agent.player_slot)), ("status", status.into()),
        ("controller", "skirmish_ai".into()), ("game_frame", number(agent.game_frame)),
        ("captured_utc", utc(agent.captured_unix_ms).into()),
        ("personality", Json::object([
            ("name", optional_text(personality.map(|value| value.name))),
            ("personality_type", optional_text(personality.map(|value| value.personality_type).filter(|value| !value.is_empty()))),
            ("ui_name", optional_text(personality.map(|value| value.ui_name).filter(|value| !value.is_empty()))),
            ("asset_instance_id", hex32(agent.personality_asset_instance_id)),
        ])),
        ("directive", Json::object([
            ("name", optional_text(directive_name(agent.strategic_directive))),
            ("raw", number(agent.strategic_directive)),
        ])),
        ("strategy", Json::object([
            ("phase", phase.into()), ("enabled", agent.strategy_enabled.into()),
            ("next_evaluation_in_frames", number(agent.state_next_evaluation_frame.saturating_sub(agent.game_frame))),
            ("next_manager_update_in_frames", number(agent.strategy_next_frame.saturating_sub(agent.game_frame))),
            ("eligible_state_count", number(agent.states.iter().filter(|state| state.score.is_finite() && state.score > 0.0 && state.flags == 0).count())),
            ("top_candidates", ranked_states(&agent.states, StateKind::Strategic, 12)),
            ("targets", Json::Array(targets)), ("active_tactics", Json::Array(tactics)),
        ])),
        ("economy", Json::object([
            ("active_budget_state", current_budget.map_or(Json::Null, |state| rough_state(state, StateKind::Budget))),
            ("budget_candidates", ranked_states(&agent.budget_states, StateKind::Budget, 8)),
            ("accounts", Json::Array(accounts)),
        ])),
        ("build", Json::object([
            ("candidate_count", number(agent.build_states.iter().filter(|state| state.score.is_finite() && state.score > 0.0 && state.flags == 0).count())),
            ("top_candidates", ranked_states(&agent.build_states, StateKind::Build, 12)),
        ])),
    ])
}

fn raw_agent(agent: &AgentDump) -> Json {
    let accounts = agent.bank_accounts.iter().enumerate().map(|(index, account)| Json::object([
        ("index", number(index)),
        ("address", address(agent.skirmish_ai.wrapping_add(0x0C + index as u32 * 36))),
        ("raw_words", Json::Array(account.words.iter().copied().map(hex32).collect())),
    ])).collect();
    let targets = agent.targets.iter().enumerate().map(|(index, target)| Json::object([
        ("index", number(index)), ("record", address(target.record)),
        ("state_wrapper", address(target.state_wrapper)), ("tactic", address(target.tactic)),
        ("object_id", number(target.object_id)), ("object_id_hex", hex32(target.object_id)),
        ("status_bits", hex32(target.status_bits)), ("state_selection_frame", number(target.state_selection_frame)),
        ("state_definition", address(target.state_definition)), ("state_asset_id", address(target.state_asset_id)),
        ("state_asset_type_id", hex32(target.state_asset_type_id)),
        ("state_asset_instance_id", hex32(target.state_asset_instance_id)),
    ])).collect();
    let tactics = agent.tactics.iter().enumerate().map(|(index, tactic)| Json::object([
        ("index", number(index)), ("instance", address(tactic.instance)),
        ("vtable", address(tactic.words[0])), ("vtable_rva", hex32(tactic.vtable_rva)),
        ("target", address(tactic.words[10])), ("skirmish_ai", address(tactic.words[11])),
        ("raw_words", Json::Array(tactic.words.iter().copied().map(hex32).collect())),
    ])).collect();
    Json::object([
        ("player_id", number(agent.player_slot)),
        ("capture_sequence", number(agent.capture_sequence)),
        ("captured_unix_ms", number(agent.captured_unix_ms)),
        ("captured_utc", utc(agent.captured_unix_ms).into()),
        ("flags", flags(agent.flags)), ("game_frame", number(agent.game_frame)),
        ("addresses", Json::object([
            ("image_base", address(agent.image_base)), ("skirmish_ai", address(agent.skirmish_ai)),
            ("bank", address(agent.bank)), ("build_manager", address(agent.build_manager)),
            ("build_state_manager", address(agent.build_state_manager)),
            ("strategic_state_manager", address(agent.strategic_state_manager)),
            ("strategy_manager", address(agent.strategy_manager)), ("economy_manager", address(agent.economy_manager)),
            ("player", address(agent.player)), ("army_definition", address(agent.army_definition)),
            ("personality_ref", address(agent.personality_ref)),
            ("personality_definition", address(agent.personality_definition)),
            ("player_list", address(agent.player_list)), ("player_ai", address(agent.player_ai)),
            ("player_skirmish_ai", address(agent.player_skirmish_ai)),
        ])),
        ("player_identity", Json::object([
            ("kind", "player_list_slot".into()), ("slot", number(agent.player_slot)),
            ("lookup_key", hex32(agent.player_lookup_key)), ("control_type", number(agent.player_control_type)),
            ("active_count", number(agent.player_list_active_count)), ("raw_flag_106", number(agent.player_raw_flag_106)),
            ("defeated", agent.player_defeated.into()), ("active_eligible", agent.player_active_eligible.into()),
            ("is_current_or_local", agent.player_is_current_or_local.into()),
        ])),
        ("runtime", Json::object([
            ("disabled", agent.disabled.into()), ("last_update_frame", number(agent.last_update_frame)),
            ("timer", number(agent.timer)), ("strategic_directive", number(agent.strategic_directive)),
            ("state_next_evaluation_frame", number(agent.state_next_evaluation_frame)),
            ("strategy_enabled", agent.strategy_enabled.into()), ("strategy_interval", number(agent.strategy_interval)),
            ("strategy_next_frame", number(agent.strategy_next_frame)),
        ])),
        ("personality", Json::object([
            ("asset_id", address(agent.personality_asset_id)),
            ("asset_type_id", hex32(agent.personality_asset_type_id)),
            ("asset_instance_id", hex32(agent.personality_asset_instance_id)),
            ("update_interval", float(agent.personality_update_interval)),
            ("update_interval_bits", hex32(agent.personality_update_interval_bits)),
            ("compatible_factions", Json::object([("count", number(agent.compatible_faction_count)), ("address", address(agent.compatible_factions))])),
            ("weighted_structures", Json::object([("count", number(agent.weighted_structure_count)), ("address", address(agent.weighted_structures))])),
            ("strategic_definitions", Json::object([("count", number(agent.strategic_definition_count)), ("address", address(agent.strategic_definitions))])),
            ("budget_definitions", Json::object([("count", number(agent.budget_definition_count)), ("address", address(agent.budget_definitions))])),
            ("build_definitions", Json::object([("count", number(agent.build_definition_count)), ("address", address(agent.build_definitions))])),
        ])),
        ("state_vectors", Json::object([
            ("strategic", raw_state_vector(agent.state_begin, agent.state_end, agent.state_capacity_end,
                agent.state_total_count, agent.state_capacity_count, &agent.states)),
            ("budget", raw_state_vector(agent.budget_state_begin, agent.budget_state_end, agent.budget_state_capacity_end,
                agent.budget_state_total_count, agent.budget_state_capacity_count, &agent.budget_states)),
            ("build", raw_state_vector(agent.build_state_begin, agent.build_state_end, agent.build_state_capacity_end,
                agent.build_state_total_count, agent.build_state_capacity_count, &agent.build_states)),
        ])),
        ("bank", Json::object([
            ("current_budget", address(agent.current_budget)), ("accounts", Json::Array(accounts)),
        ])),
        ("targets", Json::object([
            ("manager", address(agent.target_manager)), ("begin", address(agent.target_begin)),
            ("total_count", number(agent.target_total_count)), ("exported_count", number(agent.target_count)),
            ("items", Json::Array(targets)),
        ])),
        ("tactics", Json::object([
            ("manager", address(agent.tactic_manager)), ("begin", address(agent.tactic_begin)),
            ("end", address(agent.tactic_end)), ("capacity_end", address(agent.tactic_capacity_end)),
            ("total_count", number(agent.tactic_total_count)), ("exported_count", number(agent.tactic_count)),
            ("capacity_count", number(agent.tactic_capacity_count)), ("items", Json::Array(tactics)),
        ])),
    ])
}

impl PlaneDump {
    pub fn json(&self) -> String {
        let rough_players = self.agents.iter().map(rough_agent).collect();
        let raw_agents = self.agents.iter().map(raw_agent).collect();
        let root = Json::object([
            ("schema", "ra3mcp.ai-plane.v3".into()),
            ("rough_snapshot", Json::object([
                ("exported_utc", utc(self.exported_unix_ms).into()),
                ("player_count", number(self.agents.len())),
                ("players", Json::Array(rough_players)),
            ])),
            ("raw_data", Json::object([
                ("target", Json::object([
                    ("executable", "ra3_1.12.game".into()),
                    ("sha256", "dc54c54aa40d48bf2605cd5616f01e19fb1b3859b9dbe03dfa6dc28e6921e4b0".into()),
                    ("pid", number(self.pid)),
                ])),
                ("plane", Json::object([
                    ("generation", number(self.generation)), ("skipped_captures", number(self.skipped_captures)),
                    ("rejected_identities", number(self.rejected_identities)),
                    ("agent_slot_mask", hex32(self.agent_slot_mask)),
                    ("exported_unix_ms", number(self.exported_unix_ms)),
                    ("exported_utc", utc(self.exported_unix_ms).into()),
                ])),
                ("agents", Json::Array(raw_agents)),
            ])),
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

    fn put32(out: &mut Vec<u8>, value: u32) { out.extend_from_slice(&value.to_le_bytes()); }
    fn put64(out: &mut Vec<u8>, value: u64) { out.extend_from_slice(&value.to_le_bytes()); }

    fn response() -> ([u8; RESPONSE_HEADER_SIZE], Vec<u8>) {
        let mut record = Vec::new();
        put32(&mut record, 0);
        for value in [1u32 << 9, 9, 42] { put32(&mut record, value); }
        put64(&mut record, 1234);
        for value in [0x1234_0000, 0, 0, 0, 0, 0x2345_0000, 0, 0, 0, 0, 0, 0x0040_0000] { put32(&mut record, value); }
        for value in [3, 0, 0, 1, 0, 0x1234_0000, 1, 0, 0, 0, 1] { put32(&mut record, value); }
        for value in [0, 0, 0, 1, 60, 1, 30, 50, 0] { put32(&mut record, value); }
        for value in [0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0, 0x7B9D_A0E6] { put32(&mut record, value); }
        for value in [0, 0, 0, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] { put32(&mut record, value); }
        for _ in 0..BANK_ACCOUNT_COUNT * BANK_ACCOUNT_WORDS { put32(&mut record, 0); }
        for _ in 0..11 { put32(&mut record, 0); }
        assert_eq!(record.len(), AGENT_PREFIX_SIZE);
        for value in [0x7654_0000, 0.5f32.to_bits(), 0, 0x7000_0000, 0x7100_0000, 0, 0x7C55_E8D8] { put32(&mut record, value); }
        let length = record.len() as u32;
        record[0..4].copy_from_slice(&length.to_le_bytes());
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
    fn parses_v3_and_emits_rough_and_raw_regions() {
        let (header, payload) = response();
        let dump = parse_response(88, 1, &header, &payload).unwrap();
        assert_eq!(dump.generation, 7);
        assert_eq!(dump.agents[0].player_slot, 3);
        assert_eq!(dump.agents[0].states[0].score, 0.5);
        let json = dump.json();
        assert!(json.contains("ra3mcp.ai-plane.v3"));
        assert!(json.contains("\"rough_snapshot\""));
        assert!(json.contains("\"raw_data\""));
        assert!(json.contains("BalancedDefense"));
        assert!(json.contains("1SovietBalanced"));
        assert_eq!(utc(0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn rejects_truncation_and_oversized_counts() {
        let (header, mut payload) = response();
        payload[220..224].copy_from_slice(&65u32.to_le_bytes());
        assert!(parse_response(88, 1, &header, &payload).is_err());
        assert!(parse_response(88, 2, &header, &payload).is_err());
    }
}

use super::*;

impl MatchEngine {

    /// ガードまたはカットインの意思決定をプロンプト
    pub(super) fn prompt_guard_or_cutin(&mut self, defender_pid: &str) {
        let opp_ps = match self.players.get(defender_pid) {
            Some(p) => p,
            None => {
                self.proceed_to_action_resolution(None);
                return;
            }
        };

        // ガード可能カード（フィールドのACTIVE状態キャラ、攻撃側がブレットならガード不可）
        let cutin_or_disguise_used = self.action_context.as_ref().and_then(|c| c.get("defenderUsedCutin")).and_then(|v| v.as_bool()).unwrap_or(false)
            || self.action_context.as_ref().and_then(|c| c.get("cutinOrDisguiseUsed")).and_then(|v| v.as_bool()).unwrap_or(false);
        let contact_target_id = self.action_context.as_ref().and_then(|c| c.get("targetId")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let attacker_pid = self.action_context.as_ref().and_then(|c| c.get("attackerPlayerId")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let attacker_actor_id = self.action_context.as_ref().and_then(|c| c.get("actorId")).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let attacker_has_bullet = self.effective_bullet(&attacker_pid, &attacker_actor_id);
        let attacker_forces_guard = self.attacker_forces_guard(&attacker_pid, &attacker_actor_id);
        let is_case_attack = opp_ps.r#case.as_ref().map(|c| c.instance_id == contact_target_id).unwrap_or(false);

        let mut guards: Vec<String> = if attacker_has_bullet {
            vec![]
        } else {
            opp_ps.field.iter()
                .filter(|c| c.orientation == CardOrientation::ACTIVE)
                .map(|c| c.instance_id.clone())
                .collect()
        };
        // 「このキャラはガードできる場合、必ずガードする」を持つキャラがいる場合、そのキャラが必ずガードする
        let forced_guards: Vec<String> = opp_ps.field.iter()
            .filter(|c| c.must_guard_this_turn && guards.contains(&c.instance_id))
            .map(|c| c.instance_id.clone())
            .collect();
        let char_forces_guard = !forced_guards.is_empty();
        if char_forces_guard {
            guards = forced_guards;
        }
        let must_guard = (attacker_forces_guard || char_forces_guard) && !guards.is_empty();

        let (cutins, disguises, disguise_target_id) = if !cutin_or_disguise_used && !is_case_attack {
            self.collect_cutin_and_disguise_options(defender_pid, &contact_target_id)
        } else {
            (vec![], vec![], None)
        };

        if guards.is_empty() {
            if is_case_attack {
                self.proceed_to_action_resolution(None);
                return;
            }
            // 防衛側にガード可能なキャラがいない場合は、カットインフェーズへ（APの低い方から）
            if let Some(ctx) = self.action_context.as_mut() {
                ctx["guardResolved"] = json!(true);
                ctx["consecutivePasses"] = json!(0);
            }
            self.start_cutin_phase();
            return;
        }

        let dec_id = self.generate_decision_id();
        self.pending_decision = Some(json!({
            "decisionId": dec_id,
            "revision": self.revision,
            "playerId": defender_pid,
            "kind": "guard",
            "options": {
                "guardCardIds": guards,
                "cutinCardIds": [],
                "disguiseOptions": [],
                "disguiseTargetId": null,
                "mustGuard": must_guard
            }
        }));
    }

    /// カットインフェーズを開始し、コンタクト中キャラの現在APが低い方のプレイヤーから確認する
    /// （APが同じ場合は攻撃側＝ターンプレイヤーから）
    pub(super) fn start_cutin_phase(&mut self) {
        let (attacker_pid, defender_pid, actor_id, target_id, guard_id) = {
            let ctx = match self.action_context.as_ref() {
                Some(c) => c,
                None => return,
            };
            (
                ctx.get("attackerPlayerId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("guardCardId").and_then(|v| v.as_str()).map(|s| s.to_string()),
            )
        };

        let def_char_id = guard_id.clone().unwrap_or(target_id);
        let att_ap = self.players.get(&attacker_pid)
            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == actor_id))
            .map(|c| self.get_effective_ap(&attacker_pid, c))
            .unwrap_or(0);
        let def_ap = self.players.get(&defender_pid)
            .and_then(|ps| ps.field.iter().find(|c| c.instance_id == def_char_id))
            .map(|c| self.get_effective_ap(&defender_pid, c))
            .unwrap_or(0);

        let first_pid = if def_ap < att_ap && !defender_pid.is_empty() {
            defender_pid
        } else if !attacker_pid.is_empty() {
            attacker_pid
        } else {
            defender_pid
        };

        if first_pid.is_empty() {
            self.proceed_to_action_resolution(guard_id.as_deref());
        } else {
            self.prompt_contact_cutin(&first_pid);
        }
    }

    pub(super) fn collect_cutin_and_disguise_options(&self, pid: &str, contact_char_id: &str) -> (Vec<Value>, Vec<Value>, Option<String>) {
        let ps = match self.players.get(pid) {
            Some(p) => p,
            None => return (vec![], vec![], None),
        };
        let cond_ctx = self.build_cond_info(pid, None, CardOrientation::ACTIVE, None);
        let cutins: Vec<Value> = if self.turn_cutin_blocked_players.contains(pid) {
            vec![]
        } else {
            ps.hand.iter()
                .filter_map(|c| {
                    let def = self.catalog.get(&c.definition_id)?;
                    let clauses = extract_clauses(&def.description, TriggerKind::Cutin, def.card_type);
                    let any_ok = !clauses.is_empty()
                        && clauses.iter().any(|cl| check_clause_conditions(cl, &cond_ctx).is_ok());
                    if any_ok {
                        Some(json!({
                            "instanceId": c.instance_id,
                            "definitionId": c.definition_id,
                            "name": self.card_name_by_def(&c.definition_id),
                            "desc": def.description.clone(),
                        }))
                    } else {
                        None
                    }
                })
                .collect()
        };

        let mut disguises: Vec<Value> = vec![];
        let mut disguise_target_id: Option<String> = None;
        if !self.turn_disguise_blocked_players.contains(pid) && ps.field.iter().any(|c| c.instance_id == contact_char_id) {
            disguise_target_id = Some(contact_char_id.to_string());
            let effective_file = ps.effective_file_count() as i32;
            let case_def = ps.r#case.as_ref().and_then(|c| self.catalog.get(&c.definition_id));
            let case_colors = case_def.map(|d| normalize_color_set(&d.colors)).unwrap_or_default();
            for c in &ps.hand {
                if let Some(c_def) = self.catalog.get(&c.definition_id) {
                    if c_def.card_type == CardType::CHARACTER
                        && (c_def.description.contains("【変装】") || c_def.description.contains("【変装時】"))
                    {
                        let disguise_clauses = extract_clauses(&c_def.description, TriggerKind::DisguiseCond, c_def.card_type);
                        let cond_ok = disguise_clauses.is_empty()
                            || disguise_clauses.iter().any(|cl| check_disguise_conditions(cl, &cond_ctx).is_ok());
                        let card_colors = normalize_color_set(&c_def.colors);
                        let color_ok = card_colors.is_empty() || case_colors.is_empty() || card_colors.is_subset(&case_colors);
                        if cond_ok && c_def.level <= effective_file && color_ok {
                            disguises.push(json!({
                                "instanceId": c.instance_id,
                                "definitionId": c.definition_id,
                                "name": self.card_name_by_def(&c.definition_id),
                                "desc": c_def.description.clone(),
                            }));
                        }
                    }
                }
            }
        }
        (cutins, disguises, disguise_target_id)
    }

    pub(super) fn prompt_contact_cutin(&mut self, pid: &str) {
        if self.is_contact_aborted() {
            let att_pid = self
                .action_context
                .as_ref()
                .and_then(|c| c.get("attackerPlayerId"))
                .and_then(|v| v.as_str())
                .unwrap_or(&self.turn_player_id)
                .to_string();
            self.action_context = None;
                self.contact_ap_buffs.clear();
            self.log("⚠️ コンタクト中のキャラが現場を離れたためコンタクトを終了します");
            self.prompt_main_action(&att_pid);
            return;
        }
        let (attacker_pid, defender_pid, actor_id, target_id, guard_id, attacker_used, defender_used, consecutive_passes) = {
            let ctx = match self.action_context.as_ref() {
                Some(c) => c,
                None => return,
            };
            (
                ctx.get("attackerPlayerId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("actorId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.get("guardCardId").and_then(|v| v.as_str()).map(|s| s.to_string()),
                ctx.get("attackerUsedCutin").and_then(|v| v.as_bool()).unwrap_or(false),
                ctx.get("defenderUsedCutin").and_then(|v| v.as_bool()).unwrap_or(false),
                ctx.get("consecutivePasses").and_then(|v| v.as_u64()).unwrap_or(0),
            )
        };

        let is_attacker = pid == attacker_pid;
        let already_used = if is_attacker { attacker_used } else { defender_used };
        let other_pid = if is_attacker { defender_pid.clone() } else { attacker_pid.clone() };
        let other_used = if is_attacker { defender_used } else { attacker_used };

        let contact_char_id = if is_attacker {
            actor_id
        } else {
            guard_id.clone().unwrap_or(target_id)
        };

        let (cutins, disguises, disguise_target_id) = if !already_used {
            self.collect_cutin_and_disguise_options(pid, &contact_char_id)
        } else {
            (vec![], vec![], None)
        };

        if cutins.is_empty() && disguises.is_empty() {
            let next_passes = consecutive_passes + 1;
            if next_passes >= 2 || other_used || other_pid.is_empty() {
                self.proceed_to_action_resolution(guard_id.as_deref());
            } else {
                if let Some(ctx) = self.action_context.as_mut() {
                    ctx["consecutivePasses"] = json!(next_passes);
                }
                self.prompt_contact_cutin(&other_pid);
            }
            return;
        }

        let dec_id = self.generate_decision_id();
        self.pending_decision = Some(json!({
            "decisionId": dec_id,
            "revision": self.revision,
            "playerId": pid,
            "kind": "guard",
            "options": {
                "guardCardIds": [],
                "cutinCardIds": cutins,
                "disguiseOptions": disguises,
                "disguiseTargetId": disguise_target_id
            }
        }));
    }

    pub(super) fn handle_guard_answer(&mut self, player_id: &str, val: &Value) {
        let (guard_resolved, attacker_pid, defender_pid, saved_guard_id, attacker_used, defender_used, consecutive_passes) = {
            let ctx = self.action_context.as_ref();
            (
                ctx.and_then(|c| c.get("guardResolved")).and_then(|v| v.as_bool()).unwrap_or(false),
                ctx.and_then(|c| c.get("attackerPlayerId")).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.and_then(|c| c.get("defenderPlayerId")).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                ctx.and_then(|c| c.get("guardCardId")).and_then(|v| v.as_str()).map(|s| s.to_string()),
                ctx.and_then(|c| c.get("attackerUsedCutin")).and_then(|v| v.as_bool()).unwrap_or(false),
                ctx.and_then(|c| c.get("defenderUsedCutin")).and_then(|v| v.as_bool()).unwrap_or(false),
                ctx.and_then(|c| c.get("consecutivePasses")).and_then(|v| v.as_u64()).unwrap_or(0),
            )
        };
        let is_attacker = player_id == attacker_pid;
        let player_used = if is_attacker { attacker_used } else { defender_used };
        let other_pid = if is_attacker { defender_pid.clone() } else { attacker_pid.clone() };
        let other_used = if is_attacker { defender_used } else { attacker_used };

        // 変装使用かチェック（コンタクト時にカットインと変装を含めて合計1枚まで）
        if let Some(disguise_instance_id) = val.get("disguiseCardId").and_then(|v| v.as_str()) {
            if !player_used {
                let default_target = if is_attacker {
                    self.action_context.as_ref().and_then(|c| c.get("actorId")).and_then(|v| v.as_str()).unwrap_or("").to_string()
                } else {
                    saved_guard_id.clone().unwrap_or_else(|| {
                        self.action_context.as_ref().and_then(|c| c.get("targetId")).and_then(|v| v.as_str()).unwrap_or("").to_string()
                    })
                };
                let target_id = val.get("targetId").and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or(default_target);
                let turn = self.turn;
                let mut disguised_def_id = None;
                let mut disguised_inst_id = None;
                let mut replaced_def_id = None;

                if let Some(ps) = self.players.get_mut(player_id) {
                    if let (Some(h_idx), Some(f_idx)) = (
                        ps.hand.iter().position(|c| c.instance_id == disguise_instance_id),
                        ps.field.iter().position(|c| c.instance_id == target_id),
                    ) {
                        let mut hand_card = ps.hand.remove(h_idx);
                        let mut old_card = ps.field.remove(f_idx);
                        replaced_def_id = Some(old_card.definition_id.clone());
                        disguised_def_id = Some(hand_card.definition_id.clone());
                        disguised_inst_id = Some(hand_card.instance_id.clone());

                        hand_card.zone = Zone::FIELD;
                        hand_card.orientation = old_card.orientation;
                        hand_card.entered_turn = turn;
                        hand_card.face_up = true;
                        ps.field.push(hand_card);

                        old_card.zone = Zone::DECK;
                        old_card.face_up = false;
                        ps.deck.insert(0, old_card);
                    }
                }

                if let (Some(def_id), Some(inst_id), Some(old_def_id)) = (disguised_def_id, disguised_inst_id, replaced_def_id) {
                    if let Some(ctx) = self.action_context.as_mut() {
                        if is_attacker {
                            ctx["attackerUsedCutin"] = json!(true);
                            ctx["actorId"] = json!(inst_id.clone());
                        } else {
                            ctx["defenderUsedCutin"] = json!(true);
                            ctx["cutinOrDisguiseUsed"] = json!(true);
                            if saved_guard_id.is_some() {
                                ctx["guardCardId"] = json!(inst_id.clone());
                            } else {
                                ctx["targetId"] = json!(inst_id.clone());
                            }
                        }
                        ctx["consecutivePasses"] = json!(0);
                    }
                    let new_name = self.card_name_by_def(&def_id);
                    let old_name = self.card_name_by_def(&old_def_id);
                    self.last_disguise_replaced_name = self
                        .catalog
                        .get(&old_def_id)
                        .map(|d| d.names.join("|"))
                        .or_else(|| Some(old_name.clone()));
                    self.log(format!("🎭 {} が [{}] と入れ替えて [{}] を【変装】登場させました！", player_id, old_name, new_name));
                    let need_ui = self.trigger_on_disguise_with_ctx(player_id, &inst_id, &def_id, "GUARD");
                    if need_ui {
                        if let Some(ctx) = self.action_context.as_mut() {
                            ctx["pendingCutinTrigger"] = json!({
                                "userPid": player_id,
                                "cutinDefId": def_id,
                                "isDisguise": true
                            });
                        }
                        return;
                    }
                    if self.trigger_on_cutin_or_disguise_used(player_id, Some(&def_id), true) {
                        return;
                    }
                    if self.is_contact_aborted() {
                        self.action_context = None;
                self.contact_ap_buffs.clear();
                        self.log("⚠️ コンタクト中のキャラが現場を離れたためコンタクトを終了します");
                        self.prompt_main_action(&attacker_pid);
                        return;
                    }
                    if !guard_resolved {
                        self.prompt_guard_or_cutin(&defender_pid);
                    } else if !other_used && !other_pid.is_empty() {
                        self.prompt_contact_cutin(&other_pid);
                    } else {
                        let gid = self.action_context.as_ref().and_then(|c| c.get("guardCardId")).and_then(|v| v.as_str()).map(|s| s.to_string());
                        self.proceed_to_action_resolution(gid.as_deref());
                    }
                    return;
                }
            }
        }

        // カットイン使用かチェック（ガードされていない事件アタック時は使用不可、コンタクト時に各プレイヤー1回まで）
        if let Some(cutin_instance_id) = val.get("cutinCardId").and_then(|v| v.as_str()) {
            let contact_target_id = self.action_context.as_ref().and_then(|c| c.get("targetId")).and_then(|v| v.as_str()).unwrap_or("");
            let has_guard = self.action_context.as_ref().and_then(|c| c.get("guardCardId")).and_then(|v| v.as_str()).is_some()
                || val.get("guardCardId").and_then(|v| v.as_str()).is_some();
            let is_case_attack = self.players.get(&defender_pid)
                .and_then(|ps| ps.r#case.as_ref())
                .map(|c| c.instance_id == contact_target_id)
                .unwrap_or(false);
            let is_unblocked_case_attack = is_case_attack && !has_guard;
            if !player_used && !is_unblocked_case_attack {
                let cutin_instance_id = cutin_instance_id.to_string();
                let cutin_def_id = {
                    let ps = self.players.get_mut(player_id).unwrap();
                    if let Some(idx) = ps.hand.iter().position(|c| c.instance_id == cutin_instance_id) {
                        let mut card = ps.hand.remove(idx);
                        let def_id = card.definition_id.clone();
                        card.zone = Zone::REMOVE;
                        ps.remove.push(card);
                        Some(def_id)
                    } else {
                        None
                    }
                };

                if let Some(def_id) = cutin_def_id {
                    if let Some(ctx) = self.action_context.as_mut() {
                        if is_attacker {
                            ctx["attackerUsedCutin"] = json!(true);
                        } else {
                            ctx["defenderUsedCutin"] = json!(true);
                            ctx["cutinOrDisguiseUsed"] = json!(true);
                        }
                        ctx["consecutivePasses"] = json!(0);
                    }
                    let cutin_name = self.card_name_by_def(&def_id);
                    self.log(format!("⚡ {} が [{}] の【カットイン】を発動！", player_id, cutin_name));
                    let needs_target = self.resolve_cutin_effects(player_id, &cutin_instance_id, &def_id);
                    if needs_target {
                        if let Some(ctx) = self.action_context.as_mut() {
                            ctx["pendingCutinTrigger"] = json!({
                                "userPid": player_id,
                                "cutinDefId": def_id,
                                "isDisguise": false
                            });
                        }
                        return;
                    }
                    if self.trigger_on_cutin_or_disguise_used(player_id, Some(&def_id), false) {
                        return;
                    }
                    if self.is_contact_aborted() {
                        self.action_context = None;
                self.contact_ap_buffs.clear();
                        self.log("⚠️ コンタクト中のキャラが現場を離れたためコンタクトを終了します");
                        self.prompt_main_action(&attacker_pid);
                        return;
                    }
                    if !guard_resolved {
                        self.prompt_guard_or_cutin(&defender_pid);
                    } else if !other_used && !other_pid.is_empty() {
                        self.prompt_contact_cutin(&other_pid);
                    } else {
                        let gid = self.action_context.as_ref().and_then(|c| c.get("guardCardId")).and_then(|v| v.as_str()).map(|s| s.to_string());
                        self.proceed_to_action_resolution(gid.as_deref());
                    }
                    return;
                }
            }
        }

        if !guard_resolved {
            // ガード宣言またはスルー
            let attacker_actor_id = self.action_context.as_ref().and_then(|c| c.get("actorId")).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut guard_id = if self.effective_bullet(&attacker_pid, &attacker_actor_id) {
                None
            } else {
                val.get("guardCardId").and_then(|v| v.as_str()).map(|s| s.to_string())
            };
            if guard_id.is_none() {
                // 1. 防御側のキャラに「必ずガードする」が有効なアクティブキャラがいる場合、強制的にそのキャラでガード
                if let Some(forced_gid) = self.players.get(&defender_pid).and_then(|ps| {
                    ps.field.iter()
                        .find(|c| c.orientation == CardOrientation::ACTIVE && c.must_guard_this_turn)
                        .map(|c| c.instance_id.clone())
                }) {
                    guard_id = Some(forced_gid);
                } 
                // 2. 攻撃側の能力による強制ガード
                else if self.attacker_forces_guard(&attacker_pid, &attacker_actor_id) {
                    if let Some(forced_gid) = self.players.get(&defender_pid).and_then(|ps| {
                        ps.field.iter().find(|c| c.orientation == CardOrientation::ACTIVE).map(|c| c.instance_id.clone())
                    }) {
                        guard_id = Some(forced_gid);
                    }
                }
            }
            if let Some(ref gid) = guard_id {
                let guard_name = self.card_name_by_inst(gid);
                self.log(format!("🛡️ {} が [{}] でガードを宣言！", player_id, guard_name));
            } else {
                self.log(format!("{} はガードを行いませんでした（スルー）", player_id));
            }

            let contact_target_id = self.action_context.as_ref().and_then(|c| c.get("targetId")).and_then(|v| v.as_str()).unwrap_or("");
            let is_case_attack = self.players.get(&defender_pid)
                .and_then(|ps| ps.r#case.as_ref())
                .map(|c| c.instance_id == contact_target_id)
                .unwrap_or(false);

            if is_case_attack && guard_id.is_none() {
                self.proceed_to_action_resolution(None);
                return;
            }

            if let Some(ctx) = self.action_context.as_mut() {
                ctx["guardResolved"] = json!(true);
                ctx["guardCardId"] = json!(guard_id);
                ctx["consecutivePasses"] = json!(0);
            }
            self.start_cutin_phase();
            return;
        }

        // コンタクト中のカットイン/変装ウィンドウでのスルー
        let next_passes = consecutive_passes + 1;
        if next_passes >= 2 || other_used || other_pid.is_empty() {
            self.proceed_to_action_resolution(saved_guard_id.as_deref());
        } else {
            if let Some(ctx) = self.action_context.as_mut() {
                ctx["consecutivePasses"] = json!(next_passes);
            }
            self.prompt_contact_cutin(&other_pid);
        }
    }

    pub(super) fn handle_cutin_answer(&mut self, player_id: &str, val: &Value) {
        self.handle_guard_answer(player_id, val);
    }

    pub(super) fn proceed_to_action_resolution(&mut self, guard_id: Option<&str>) {
        if self.is_contact_aborted() {
            let att_pid = self
                .action_context
                .as_ref()
                .and_then(|c| c.get("attackerPlayerId"))
                .and_then(|v| v.as_str())
                .unwrap_or(&self.turn_player_id)
                .to_string();
            self.action_context = None;
                self.contact_ap_buffs.clear();
            self.log("⚠️ コンタクト中のキャラが現場を離れたためコンタクトを終了します");
            if !self.drain_pending_clause_queue() {
                self.prompt_main_action(&att_pid);
            }
            return;
        }
        let ctx = match self.action_context.take() {
            Some(c) => c,
            None => return,
        };

        let attacker_pid = ctx["attackerPlayerId"].as_str().unwrap().to_string();
        let defender_pid = ctx["defenderPlayerId"].as_str().unwrap().to_string();
        let actor_id = ctx["actorId"].as_str().unwrap().to_string();
        let target_id = ctx["targetId"].as_str().unwrap().to_string();

        let is_case_action;
        let mut target_field_card_id = None;

        if let Some(gid) = guard_id {
            if let Some(defender_ps) = self.players.get_mut(&defender_pid) {
                if let Some(g_card) = defender_ps.field.iter_mut().find(|c| c.instance_id == gid) {
                    g_card.orientation = CardOrientation::SLEEP;
                }
            }
            is_case_action = false;
            target_field_card_id = Some(gid.to_string());
        } else {
            let defender_ps = self.players.get(&defender_pid).unwrap();
            if let Some(ref c) = defender_ps.r#case {
                if target_id == c.instance_id {
                    is_case_action = true;
                } else {
                    is_case_action = false;
                    target_field_card_id = Some(target_id.clone());
                }
            } else {
                is_case_action = false;
                target_field_card_id = Some(target_id.clone());
            }
        }

        if is_case_action {
            // B03080 (BANG): このキャラがアクション［事件］したとき、ターン1回アクティブにする
            let mut b03080_target: Option<(String, String)> = None;
            if let Some(att_ps) = self.players.get(&attacker_pid) {
                if let Some(att_c) = att_ps.field.iter().find(|c| c.instance_id == actor_id) {
                    for sc in &att_c.set_cards {
                        if let Some(sdef) = self.catalog.get(&sc.definition_id) {
                            if sdef.description.contains("アクション［事件］したとき、このキャラをアクティブにする")
                                && self.can_use_clause_this_turn(&sc.instance_id, &sdef.description)
                            {
                                b03080_target = Some((sc.instance_id.clone(), sdef.description.clone()));
                                break;
                            }
                        }
                    }
                }
            }
            if let Some((sid, sdesc)) = b03080_target {
                if let Some(att_ps) = self.players.get_mut(&attacker_pid) {
                    if let Some(att_c) = att_ps.field.iter_mut().find(|c| c.instance_id == actor_id) {
                        att_c.orientation = CardOrientation::ACTIVE;
                    }
                }
                self.mark_clause_used_this_turn(&sid, &sdesc);
                let aname = self.card_name_by_inst(&actor_id);
                self.log(format!("💥 [BANG] の効果で [{}] をアクティブにしました！", aname));
            }

            let blocks_opp_hirameki = self.players.get(&attacker_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == actor_id))
                .and_then(|c| self.catalog.get(&c.definition_id))
                .map(|d| d.description.contains("アクション［事件］したとき、アクション終了時まで相手の【ヒラメキ】は発動しない"))
                .unwrap_or(false);
            if blocks_opp_hirameki {
                self.action_hirameki_blocked_player = Some(defender_pid.clone());
            }
            let has_evidence = self.players.get(&defender_pid).map(|p| !p.evidence.is_empty()).unwrap_or(false);
            if has_evidence {
                let resume_tag = format!("CASE_ATTACK:{}:{}", attacker_pid, actor_id);
                let prev_rem_len = self.players.get(&defender_pid).map(|p| p.remove.len()).unwrap_or(0);
                let (opened_hirameki_ui, block_gain) =
                    self.remove_evidence_and_trigger_hirameki(&defender_pid, 1, &resume_tag);
                let removed_card_name = self.players.get(&defender_pid)
                    .and_then(|p| if p.remove.len() > prev_rem_len { p.remove.get(prev_rem_len) } else { None })
                    .map(|c| self.card_name_by_def(&c.definition_id))
                    .unwrap_or_else(|| "証拠カード".to_string());
                if block_gain {
                    self.log(format!("💥 事件への攻撃ヒット！ {} の証拠から [{}] をリムーブしました（ヒラメキにより {} は証拠を獲得できません）", defender_pid, removed_card_name, attacker_pid));
                } else {
                    self.gain_evidence(&attacker_pid, 1);
                    self.log(format!("💥 事件への攻撃ヒット！ {} の証拠から [{}] をリムーブし、{} が証拠を1つ獲得！", defender_pid, removed_card_name, attacker_pid));
                }
                self.action_hirameki_blocked_player = None;
                if opened_hirameki_ui {
                    return;
                }
                if !block_gain {
                    self.trigger_on_evidence_gain(&attacker_pid, Some(&actor_id), true);
                    if self.pending_decision.is_some() {
                        return;
                    }
                }
            } else {
                self.action_hirameki_blocked_player = None;
                self.log(format!("💥 事件への攻撃！ しかし {} の証拠はすでに0でした", defender_pid));
            }
        } else if let Some(def_cid) = target_field_card_id {
            // コンタクト時トリガーの解決（バトルAP比較前）
            self.trigger_on_contact(&attacker_pid, &actor_id, &defender_pid, &def_cid);

            let actor_def_id = self.players.get(&attacker_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == actor_id).map(|c| c.definition_id.clone()));
            let def_def_id = self.players.get(&defender_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == def_cid).map(|c| c.definition_id.clone()));

            let actor_ap = self.players.get(&attacker_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == actor_id))
                .map(|c| self.get_effective_ap_with_contact(&attacker_pid, c, true))
                .unwrap_or(0);
            let def_ap = self.players.get(&defender_pid)
                .and_then(|p| p.field.iter().find(|c| c.instance_id == def_cid))
                .map(|c| self.get_effective_ap_with_contact(&defender_pid, c, true))
                .unwrap_or(0);

            if let (Some(a_def_id), Some(d_def_id)) = (actor_def_id, def_def_id) {
                let actor_name = self.card_name_by_def(&a_def_id);
                let def_name = self.card_name_by_def(&d_def_id);
                if !self.contact_ap_buffs.is_empty() {
                    let buffs = format!("{:?}", self.contact_ap_buffs);
                    self.debug_log(format!("[DEBUG][CONTACT_END] contact_ap_buffs expired: {}", buffs));
                }
                self.contact_ap_buffs.clear();

                if actor_ap >= def_ap {
                    if let Some((rem_def, removed_sets)) = self.move_field_char_to_remove(&defender_pid, &def_cid, true, false) {
                        self.log(format!("💥 バトル決着: [{}] (AP{}) vs [{}] (AP{}) ➔ {} の [{}] をリムーブ！", actor_name, actor_ap, def_name, def_ap, defender_pid, def_name));
                        self.trigger_on_remove_char(&defender_pid, &def_cid, &rem_def, false);
                        for (sid, sdid) in removed_sets {
                            self.trigger_set_card_on_remove(&defender_pid, &sid, &sdid);
                        }
                        // コンタクト勝利時トリガーの解決
                        self.contact_removed_actors_this_turn.insert(actor_id.clone());
                        self.trigger_on_contact_win(&attacker_pid, &actor_id, &a_def_id);
                    } else {
                        self.log(format!("🛡️ [{}] は身代わり/効果によりコンタクトでのリムーブを免れました！", def_name));
                    }
                } else {
                    self.log(format!("🛡️ バトル決着: [{}] (AP{}) vs [{}] (AP{}) ➔ AP不足のためリムーブできず（守備成功）", actor_name, actor_ap, def_name, def_ap));
                }
            }
        }

        // コンタクト終了：カットイン等によるコンタクト中のAP増減はここで切れる
        if !self.contact_ap_buffs.is_empty() {
            let buffs = format!("{:?}", self.contact_ap_buffs);
            self.debug_log(format!("[DEBUG][CONTACT_END] contact_ap_buffs expired: {}", buffs));
        }
        self.contact_ap_buffs.clear();
        if !self.pending_decision.is_some() && !self.drain_pending_clause_queue() {
            self.prompt_main_action(&attacker_pid);
        }
    }

    pub(super) fn contact_char_matches_req(&self, player_id: &str, req_phrase: &str) -> bool {
        let Some(cid) = self.get_contact_char_id_for_player(player_id) else {
            return false;
        };
        let Some(ps) = self.players.get(player_id) else {
            return false;
        };
        let Some(cc) = ps.field.iter().find(|c| c.instance_id == cid) else {
            return false;
        };
        let Some(cdef) = self.catalog.get(&cc.definition_id) else {
            return false;
        };
        let eff_ap = self.get_effective_ap(player_id, cc);
        let mut eff_def = cdef.clone();
        eff_def.level = cdef.level + cc.temp_level_delta;
        let branches = crate::effects::parse_branches_from_phrase(req_phrase, CardType::CHARACTER);
        branches.iter().any(|b| {
            if let Some(ref nm) = b.name_req {
                if has_instance_name_match(cc, &eff_def, nm) {
                    let mut b_no_name = b.clone();
                    b_no_name.name_req = None;
                    return branch_matches(&eff_def, eff_ap, &b_no_name);
                }
                return false;
            }
            branch_matches(&eff_def, eff_ap, b)
        })
    }

    pub(super) fn apply_ap_buff_to_contact_char(&mut self, player_id: &str, buff_source_name: &str, amt: i32) {
        if let Some(cid) = self.get_contact_char_id_for_player(player_id) {
            if let Some(ps) = self.players.get_mut(player_id) {
                if let Some(c) = ps.field.iter_mut().find(|c| c.instance_id == cid) {
                    *self.contact_ap_buffs.entry(c.instance_id.clone()).or_insert(0) += amt;
                    let target_name = self
                        .catalog
                        .get(&c.definition_id)
                        .and_then(|d| d.names.first().cloned())
                        .unwrap_or_default();
                    self.log(format!("⚡ [{}] の効果で [{}] のAP +{}", buff_source_name, target_name, amt));
                }
            }
        }
    }

    /// カットイン効果を解決する（対象選択が必要な場合は true を返す）
    pub(super) fn resolve_cutin_effects(&mut self, player_id: &str, cutin_instance_id: &str, definition_id: &str) -> bool {
        let Some(def) = self.catalog.get(definition_id).cloned() else {
            return false;
        };
        let clauses = extract_clauses(&def.description, TriggerKind::Cutin, def.card_type);
        if clauses.is_empty() {
            return false;
        }
        let card_name = self.card_name_by_def(definition_id);

        for clause in clauses {
            let cond_check = {
                let ctx = self.build_cond_info(player_id, Some(cutin_instance_id), CardOrientation::ACTIVE, None);
                check_clause_conditions(&clause, &ctx)
            };
            if let Err(reason) = cond_check {
                self.log(format!("⚠️ [{}] のカットイン条件未達: {}", card_name, reason));
                continue;
            }
            let cost = parse_clause_cost(&clause);
            if !self.pay_clause_cost(player_id, cutin_instance_id, &cost) {
                self.log(format!("⚠️ [{}] のカットインコスト不足", card_name));
                continue;
            }
            let clean = crate::effects::strip_parens(&clause);
            let body = crate::effects::strip_leading_tags(&clean).trim();

            // 1) 「自分の現場に空きがある場合、スリープ状態で登場させる」系カットイン
            if clean.contains("自分の現場に空きがある場合、スリープ状態で登場させる") {
                if let Some(ps) = self.players.get_mut(player_id) {
                    if ps.field.len() < 5 {
                        if let Some(idx) = ps.remove.iter().position(|c| c.instance_id == cutin_instance_id) {
                            let mut c = ps.remove.remove(idx);
                            c.zone = Zone::FIELD;
                            c.orientation = CardOrientation::SLEEP;
                            c.entered_turn = self.turn;
                            c.face_up = true;
                            ps.field.push(c);
                            self.log(format!("⚡ [{}] のカットイン効果で自身の現場にスリープ登場！", card_name));
                            continue;
                        }
                    }
                }
            }

            // 2) B10091: 「〜に【カットイン】した場合、このキャラをリムーブエリアから登場させる」
            if body.contains("に【カットイン】した場合、このキャラをリムーブエリアから登場させる") {
                if let Some((req_target, _)) = body.split_once("に【カットイン】した場合、") {
                    if self.contact_char_matches_req(player_id, req_target.trim()) {
                        let turn = self.turn;
                        let mut deployed = false;
                        if let Some(ps) = self.players.get_mut(player_id) {
                            if ps.field.len() < 5 {
                                if let Some(idx) = ps.remove.iter().position(|c| c.instance_id == cutin_instance_id) {
                                    let mut c = ps.remove.remove(idx);
                                    c.zone = Zone::FIELD;
                                    c.orientation = CardOrientation::ACTIVE;
                                    c.entered_turn = turn;
                                    c.face_up = true;
                                    ps.field.push(c);
                                    deployed = true;
                                }
                            }
                        }
                        if deployed {
                            self.self_entered_field_players_this_turn.insert(player_id.to_string());
                            self.log(format!("⚡ [{}] のカットイン効果でリムーブエリアから現場に登場！", card_name));
                            if self.resolve_character_enter_field_with_source(
                                player_id,
                                cutin_instance_id,
                                definition_id,
                                CardOrientation::ACTIVE,
                                Some(def.level),
                                "GUARD",
                            ) {
                                return true;
                            }
                        }
                    }
                }
                continue;
            }

            // 3) B03039: 「AP＋1000、相手の現場にいるキャラに裏向きでセットされているカードを1枚選び、リムーブしてもよい。そうした場合、代わりにAP＋3000」
            if clean.contains("代わりにAP＋") && clean.contains("裏向きでセットされているカード") {
                let base_ap = crate::effects::parse_signed_ap(
                    clean.split("相手の現場").next().unwrap_or(&clean),
                    true,
                )
                .unwrap_or(1000);
                let alt_ap = clean
                    .split("代わりに")
                    .nth(1)
                    .and_then(|s| crate::effects::parse_signed_ap(s, true))
                    .unwrap_or(3000);
                self.apply_ap_buff_to_contact_char(player_id, &card_name, base_ap);
                let bonus_diff = alt_ap - base_ap;
                let mut set_f = TargetFilter {
                    owner: OwnerScope::OppOnly,
                    zone: TargetZone::SetCards,
                    face_down_only: true,
                    count: 1,
                    up_to: true,
                    any_card_type: true,
                    branches: vec![crate::effects::TargetBranch::default()],
                    ..Default::default()
                };
                set_f.face_down_only = true;
                let ts = TargetStep {
                    action: TargetAction::RemoveSetCards(1),
                    filter: set_f,
                    bonuses: vec![],
                    follow_up: if bonus_diff != 0 {
                        vec![ImmediateOp::SelfApBuff(bonus_diff)]
                    } else {
                        vec![]
                    },
                    conditional_follow_up: None,
                    optional: true,
                    then_abort_if_skipped: false,
                    scale_by_cost_flipped_evidence: false,
                };
                if self.prompt_target_step(player_id, cutin_instance_id, &card_name, Some(def.level), &ts, None, "GUARD", None) {
                    return true;
                }
                continue;
            }

            // 4) B05040 / B08055: 手札を1枚リムーブしてレベル/APに応じたAPバフ
            if clean.contains("リムーブしたカードのレベル1につき、AP＋") {
                let per_lv = clean
                    .split("レベル1につき、")
                    .nth(1)
                    .and_then(|s| crate::effects::parse_signed_ap(s, true))
                    .unwrap_or(1000);
                let sub_plan = ParsedClausePlan {
                    immediate_before: vec![
                        ImmediateOp::OptionalDiscardHand { count: 1, filter: None },
                        ImmediateOp::CutinBuffByRemovedLevel(per_lv),
                    ],
                    ..Default::default()
                };
                if self.execute_clause_plan(player_id, cutin_instance_id, &card_name, Some(def.level), &sub_plan, "GUARD", None) {
                    return true;
                }
                continue;
            }
            if clean.contains("リムーブしたキャラのAP1000につき、AP＋") {
                let per_k = clean
                    .split("AP1000につき、")
                    .nth(1)
                    .and_then(|s| crate::effects::parse_signed_ap(s, true))
                    .unwrap_or(1000);
                let char_f = TargetFilter {
                    owner: OwnerScope::SelfOnly,
                    zone: TargetZone::Hand,
                    count: 1,
                    card_type_filter: Some(CardType::CHARACTER),
                    branches: vec![crate::effects::TargetBranch {
                        card_types: vec![CardType::CHARACTER],
                        ..Default::default()
                    }],
                    ..Default::default()
                };
                let sub_plan = ParsedClausePlan {
                    immediate_before: vec![
                        ImmediateOp::OptionalDiscardHand { count: 1, filter: Some(char_f) },
                        ImmediateOp::CutinBuffByRemovedApThousand(per_k),
                    ],
                    ..Default::default()
                };
                if self.execute_clause_plan(player_id, cutin_instance_id, &card_name, Some(def.level), &sub_plan, "GUARD", None) {
                    return true;
                }
                continue;
            }

            // 5) 条件による代替AP（「AP＋1000、Xの場合、代わりにAP＋3000」等）
            if body.contains("代わりにAP＋") {
                if let Some((before_repl, after_repl)) = body.split_once("代わりに") {
                    let alt_ap = crate::effects::parse_signed_ap(after_repl, true).unwrap_or(3000);
                    let base_ap = before_repl
                        .split('、')
                        .next()
                        .and_then(|s| crate::effects::parse_signed_ap(s, true))
                        .unwrap_or(1000);
                    let cond_raw = before_repl
                        .split_once('、')
                        .map(|(_, r)| r.trim())
                        .unwrap_or("")
                        .trim_end_matches("場合、")
                        .trim_end_matches("場合");
                    let cond_met = if let Some((treq, _)) = cond_raw.split_once("に【カットイン】") {
                        self.contact_char_matches_req(player_id, treq.trim())
                    } else if let Some(max_h) = crate::effects::extract_number_before(cond_raw, "手札が", "枚以下") {
                        self.players.get(player_id).map(|ps| ps.hand.len() <= max_h as usize).unwrap_or(false)
                    } else {
                        let ctx2 = self.build_cond_info(player_id, Some(cutin_instance_id), CardOrientation::ACTIVE, None);
                        crate::effects::eval_cond_phrase(cond_raw, &ctx2)
                    };
                    let final_ap = if cond_met { alt_ap } else { base_ap };
                    self.apply_ap_buff_to_contact_char(player_id, &card_name, final_ap);
                    if after_repl.contains("カードを") && after_repl.contains("引く") {
                        let d = parse_draw_count(after_repl).max(1);
                        if cond_met {
                            self.draw_cards(player_id, d);
                            self.log(format!("⚡ [{}] のカットイン追加効果でカードを{}枚引きました", card_name, d));
                        }
                    }
                    continue;
                }
            }

            // 6) 現場・リムーブ枚数によるスケールAP（「自分の現場/リムーブにある〜1枚につき、AP＋N」）
            if clean.contains("1枚につき、AP＋") {
                let per_card = crate::effects::parse_signed_ap(&clean, true).unwrap_or(1000);
                if let Some((scope_part, _)) = body.split_once("1枚につき、") {
                    let branches = crate::effects::parse_branches_from_phrase(scope_part, CardType::CHARACTER);
                    let cnt = if scope_part.contains("リムーブエリア") {
                        self.players.get(player_id).map(|ps| {
                            ps.remove.iter().filter(|rc| {
                                self.catalog.get(&rc.definition_id).map(|rdef| {
                                    branches.iter().any(|b| branch_matches(rdef, rdef.ap, b))
                                }).unwrap_or(false)
                            }).count()
                        }).unwrap_or(0)
                    } else {
                        self.players.get(player_id).map(|ps| {
                            ps.field.iter().filter(|fc| {
                                let fap = self.get_effective_ap(player_id, fc);
                                self.catalog.get(&fc.definition_id).map(|fdef| {
                                    branches.iter().any(|b| branch_matches(fdef, fap, b))
                                }).unwrap_or(false)
                            }).count()
                        }).unwrap_or(0)
                    };
                    let total_buff = (cnt as i32) * per_card;
                    if total_buff != 0 {
                        self.apply_ap_buff_to_contact_char(player_id, &card_name, total_buff);
                    }
                    continue;
                }
            }

            // 7) B10086 等: 「AP＋1000、〜に【カットイン】した場合 / 自分の事件が〜場合、カードを1枚引く」
            if body.contains("場合、カードを")
                && (body.contains("に【カットイン】した場合、カードを")
                    || body.contains("以外の色を持たない場合、カードを"))
            {
                if let Some((before_c, after_c)) = body.split_once("場合、") {
                    if let Some((ap_part, cond_part)) = before_c.split_once('、') {
                        let cond_met = if let Some((treq, _)) = cond_part.split_once("に【カットイン】") {
                            self.contact_char_matches_req(player_id, treq.trim())
                        } else {
                            let ctx2 = self.build_cond_info(player_id, Some(cutin_instance_id), CardOrientation::ACTIVE, None);
                            crate::effects::eval_cond_phrase(cond_part, &ctx2)
                        };
                        if let Some(ap) = crate::effects::parse_signed_ap(ap_part, true) {
                            self.apply_ap_buff_to_contact_char(player_id, &card_name, ap);
                        }
                        if cond_met {
                            let d = parse_draw_count(after_c).max(1);
                            self.draw_cards(player_id, d);
                            self.log(format!("⚡ [{}] のカットイン効果でカードを{}枚引きました", card_name, d));
                        }
                        continue;
                    }
                }
            }

            let plan = parse_effect_steps(&clause, None, def.card_type);
            if self.execute_clause_plan(player_id, cutin_instance_id, &card_name, Some(def.level), &plan, "GUARD", None) {
                return true;
            }
        }
        false
    }

    pub(super) fn trigger_on_cutin_or_disguise_used(
        &mut self,
        user_pid: &str,
        cutin_def_id: Option<&str>,
        is_disguise: bool,
    ) -> bool {
        let used_def = cutin_def_id.and_then(|did| self.catalog.get(did)).cloned();
        let pids = self.player_order.clone();
        for watch_pid in pids {
            let is_user = watch_pid == user_pid;
            let watchers: Vec<(String, String, CardOrientation, bool)> = self
                .players
                .get(&watch_pid)
                .map(|ps| {
                    let mut list: Vec<(String, String, CardOrientation, bool)> = ps
                        .field
                        .iter()
                        .map(|c| (c.instance_id.clone(), c.definition_id.clone(), c.orientation, false))
                        .collect();
                    if let Some(ref p) = ps.partner {
                        if p.zone == Zone::PARTNER {
                            list.push((p.instance_id.clone(), p.definition_id.clone(), p.orientation, true));
                        }
                    }
                    for ep in &ps.extra_partners {
                        list.push((ep.instance_id.clone(), ep.definition_id.clone(), ep.orientation, true));
                    }
                    if let Some(ref cs) = ps.r#case {
                        list.push((cs.instance_id.clone(), cs.definition_id.clone(), cs.orientation, false));
                    }
                    list
                })
                .unwrap_or_default();

            for (watcher_id, wdef_id, ori, in_partner) in watchers {
                let Some(wdef) = self.catalog.get(&wdef_id).cloned() else {
                    continue;
                };
                let watcher_name = self.card_name_by_def(&wdef_id);
                let norm_desc = wdef.description.replace("\r\n", "\n").replace('\r', "\n");
                for raw_line in norm_desc.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    let clean = strip_parens(raw_line);
                    if !clean.contains("【カットイン】を使用したとき") && !clean.contains("【カットイン】か【変装】を使用したとき") {
                        continue;
                    }
                    if clean.starts_with("【カットイン】") || clean.contains("「") {
                        continue;
                    }
                    if in_partner && wdef.card_type != CardType::PARTNER && !allows_partner_area(raw_line) {
                        continue;
                    }
                    let ctx = self.build_cond_info(&watch_pid, Some(&watcher_id), ori, None);
                    if check_clause_conditions(raw_line, &ctx).is_err() {
                        continue;
                    }
                    if !self.can_use_clause_this_turn(&watcher_id, raw_line) {
                        continue;
                    }
                    let expects_opp = clean.contains("相手が【カットイン】");
                    if expects_opp {
                        if is_user {
                            continue;
                        }
                        if is_disguise && !clean.contains("か【変装】") {
                            continue;
                        }
                    } else {
                        if !is_user || is_disguise {
                            continue;
                        }
                    }
                    if clean.contains("このキャラのコンタクト中に") && !self.is_char_in_current_contact(&watch_pid, &watcher_id) {
                        continue;
                    }
                    let contact_cid = self.get_contact_char_id_for_player(&watch_pid);
                    if clean.contains("自分の現場にいる") && clean.contains("キャラのコンタクト中に") {
                        let Some(ref ccid) = contact_cid else {
                            continue;
                        };
                        if let Some(tr) = extract_bracketed_value(&clean, "特徴［", "］") {
                            let tr_ok = self
                                .players
                                .get(&watch_pid)
                                .and_then(|ps| ps.field.iter().find(|c| c.instance_id == *ccid))
                                .and_then(|c| self.catalog.get(&c.definition_id))
                                .map(|cd| has_trait_match(cd, tr))
                                .unwrap_or(false);
                            if !tr_ok {
                                continue;
                            }
                        }
                    }
                    if let Some((_, req_cutin_part)) = clean.split_once("自分が").and_then(|(_, r)| r.split_once("の【カットイン】を使用したとき")) {
                        if req_cutin_part.contains("〚") {
                            let Some(ref cd) = used_def else {
                                continue;
                            };
                            let branches = crate::effects::parse_branches_from_phrase(req_cutin_part, CardType::CHARACTER);
                            if !branches.iter().any(|b| branch_matches(cd, cd.ap, b)) {
                                continue;
                            }
                        }
                    }

                    let cost = parse_clause_cost(raw_line);
                    if !self.pay_clause_cost(&watch_pid, &watcher_id, &cost) {
                        continue;
                    }
                    self.mark_clause_used_this_turn(&watcher_id, raw_line);
                    self.log(format!("⚡ [{}] の誘発効果（カットイン使用時）が発動！", watcher_name));

                    let body = clean.split("を使用したとき、").nth(1).unwrap_or(&clean);
                    if body.contains("そのキャラをAP＋") {
                        if let Some(ap) = crate::effects::parse_signed_ap(body, true) {
                            self.apply_ap_buff_to_contact_char(&watch_pid, &watcher_name, ap);
                        }
                        continue;
                    }
                    if body.contains("このキャラをAP＋") {
                        if let Some(ap) = crate::effects::parse_signed_ap(body, true) {
                            if let Some(ps) = self.players.get_mut(&watch_pid) {
                                if let Some(wc) = ps.field.iter_mut().find(|c| c.instance_id == watcher_id) {
                                    wc.temp_ap_bonus += ap;
                                    self.log(format!("⚡ [{}] の効果で自身のAP +{}", watcher_name, ap));
                                }
                            }
                        }
                        continue;
                    }
                    if body.contains("このキャラをアクティブにし") {
                        if let Some(ps) = self.players.get_mut(&watch_pid) {
                            if let Some(wc) = ps.field.iter_mut().find(|c| c.instance_id == watcher_id) {
                                wc.orientation = CardOrientation::ACTIVE;
                                self.log(format!("✨ [{}] をアクティブにしました！", watcher_name));
                            }
                        }
                        continue;
                    }
                    if body.contains("カードを") && body.contains("引く") && !body.contains("選び") {
                        let d = parse_draw_count(body).max(1);
                        self.draw_cards(&watch_pid, d);
                        self.log(format!("✨ [{}] の効果でカードを{}枚引きました", watcher_name, d));
                        continue;
                    }

                    let plan = parse_effect_steps(raw_line, None, wdef.card_type);
                    if self.execute_clause_plan(&watch_pid, &watcher_id, &watcher_name, Some(wdef.level), &plan, "GUARD", None) {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub(super) fn is_action_target_still_valid(&self) -> bool {
        let Some(ctx) = self.action_context.as_ref() else {
            return false;
        };
        let target_id = ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("");
        let def_pid = ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("");
        let Some(def_ps) = self.players.get(def_pid) else {
            return false;
        };
        if def_ps.r#case.as_ref().map(|c| c.instance_id == target_id).unwrap_or(false) {
            return true;
        }
        def_ps.field.iter().any(|c| c.instance_id == target_id)
    }

    pub fn get_contact_char_id_for_player(&self, player_id: &str) -> Option<String> {
        let ctx = self.action_context.as_ref()?;
        let att_pid = ctx.get("attackerPlayerId").and_then(|v| v.as_str()).unwrap_or("");
        let def_pid = ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("");
        let actor_id = ctx.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
        let target_id = ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("");
        let guard_id = ctx.get("guardCardId").and_then(|v| v.as_str());
        if player_id == att_pid && !actor_id.is_empty() {
            return Some(actor_id.to_string());
        }
        if player_id == def_pid {
            if let Some(gid) = guard_id {
                if !gid.is_empty() {
                    return Some(gid.to_string());
                }
            }
            if !target_id.is_empty() {
                let is_case = self
                    .players
                    .get(def_pid)
                    .and_then(|ps| ps.r#case.as_ref())
                    .map(|cs| cs.instance_id == target_id)
                    .unwrap_or(false);
                if !is_case {
                    return Some(target_id.to_string());
                }
            }
        }
        None
    }

    pub fn is_char_in_current_contact(&self, owner_id: &str, inst_id: &str) -> bool {
        self.get_contact_char_id_for_player(owner_id).as_deref() == Some(inst_id)
    }

    pub fn is_contact_aborted(&self) -> bool {
        let Some(ctx) = self.action_context.as_ref() else {
            return true;
        };
        let att_pid = ctx.get("attackerPlayerId").and_then(|v| v.as_str()).unwrap_or("");
        let def_pid = ctx.get("defenderPlayerId").and_then(|v| v.as_str()).unwrap_or("");
        let actor_id = ctx.get("actorId").and_then(|v| v.as_str()).unwrap_or("");
        let target_id = ctx.get("targetId").and_then(|v| v.as_str()).unwrap_or("");
        let guard_id = ctx.get("guardCardId").and_then(|v| v.as_str());

        let attacker_alive = self
            .players
            .get(att_pid)
            .map(|ps| ps.field.iter().any(|c| c.instance_id == actor_id))
            .unwrap_or(false);
        if !attacker_alive {
            return true;
        }
        if let Some(gid) = guard_id {
            if !gid.is_empty() {
                let guard_alive = self
                    .players
                    .get(def_pid)
                    .map(|ps| ps.field.iter().any(|c| c.instance_id == gid))
                    .unwrap_or(false);
                return !guard_alive;
            }
        }
        let is_case_target = self
            .players
            .get(def_pid)
            .and_then(|ps| ps.r#case.as_ref())
            .map(|cs| cs.instance_id == target_id)
            .unwrap_or(false);
        if is_case_target {
            return false;
        }
        let target_alive = self
            .players
            .get(def_pid)
            .map(|ps| ps.field.iter().any(|c| c.instance_id == target_id))
            .unwrap_or(false);
        !target_alive
    }
}

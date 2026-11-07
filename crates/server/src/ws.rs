//! WS-слой PoP - Protocol: аутентификация по Hello и диспетчеризация ClientMsg.
//! Игровая логика — в lobby.rs; здесь только транспорт и маршрутизация сообщений.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::future::BoxFuture;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use popproto_core::{ClientMsg, ErrCode, ServerMsg, PROTOCOL_VERSION};
use tokio::sync::mpsc;

use crate::game::GameCmd;
use crate::store::{PlayerRecord, StoreError};
use crate::AppState;

pub async fn upgrade(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| run_session(socket, state))
}

async fn send_msg(sink: &mut SplitSink<WebSocket, Message>, msg: &ServerMsg) -> bool {
    sink.send(Message::Binary(popproto_core::encode(msg).into()))
        .await
        .is_ok()
}

async fn send_err(sink: &mut SplitSink<WebSocket, Message>, code: ErrCode, text: &str) {
    let _ = send_msg(
        sink,
        &ServerMsg::Error {
            code,
            text: text.to_string(),
        },
    )
    .await;
}

/// Рукопожатие: первый кадр должен быть Hello с совпадающей версией протокола.
/// Возвращает запись игрока и поток чтения.
async fn handshake(
    socket: WebSocket,
    state: &AppState,
) -> Option<(
    PlayerRecord,
    SplitStream<WebSocket>,
    SplitSink<WebSocket, Message>,
)> {
    let (mut sink, mut stream) = socket.split();

    let first = tokio::time::timeout(std::time::Duration::from_secs(10), stream.next())
        .await
        .ok()??;
    let bytes = match first.ok()? {
        Message::Binary(b) => b,
        _ => return None,
    };
    let Some(ClientMsg::Hello {
        protocol,
        name,
        token,
    }) = popproto_core::decode::<ClientMsg>(&bytes)
    else {
        send_err(
            &mut sink,
            ErrCode::InvalidInput,
            "первый пакет должен быть Hello",
        )
        .await;
        return None;
    };
    if protocol != PROTOCOL_VERSION {
        send_err(
            &mut sink,
            ErrCode::OldProtocol,
            &format!("обновите клиент: сервер {PROTOCOL_VERSION}"),
        )
        .await;
        return None;
    }

    match state.store.resolve_player(&name, token.as_deref()).await {
        Ok(record) => {
            if send_msg(
                &mut sink,
                &ServerMsg::Welcome {
                    player_id: record.player_id,
                    token: record.token.clone(),
                    name: record.name.clone(),
                },
            )
            .await
            {
                tracing::info!("игрок «{}» вошёл (id {})", record.name, record.player_id);
                Some((record, stream, sink))
            } else {
                None
            }
        }
        Err(e) => {
            let (code, text) = store_error(&e);
            send_err(&mut sink, code, text).await;
            None
        }
    }
}

fn store_error(e: &StoreError) -> (ErrCode, &'static str) {
    match e {
        StoreError::InvalidName => (ErrCode::BadName, "ник: 1–16 символов"),
        StoreError::NameTaken => (ErrCode::NameTaken, "ник занят, выберите другой"),
        StoreError::BadToken => (ErrCode::BadToken, "токен устарел, войдите заново"),
        StoreError::Backend(s) => {
            tracing::error!("store backend: {s}");
            (ErrCode::InvalidInput, "хранилище недоступно")
        }
    }
}

/// Живёт всё соединение: читает ClientMsg, пишет ServerMsg через прямой канал.
async fn run_session(socket: WebSocket, state: AppState) {
    state
        .ws_clients
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let Some((record, mut stream, mut sink)) = handshake(socket, &state).await else {
        state
            .ws_clients
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        return;
    };

    let (direct_tx, mut direct_rx) = mpsc::unbounded_channel::<ServerMsg>();
    // Пересыльщик событий комнаты -> прямой канал; пересоздаётся при смене комнаты.
    let mut forwarder: Option<tokio::task::JoinHandle<()>> = None;
    let mut room_id: Option<u32> = None;

    let writer = tokio::spawn(async move {
        while let Some(msg) = direct_rx.recv().await {
            if sink
                .send(Message::Binary(popproto_core::encode(&msg).into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    while let Some(Ok(frame)) = stream.next().await {
        let bytes = match frame {
            Message::Binary(b) => b,
            Message::Text(_) | Message::Pong(_) => continue,
            Message::Close(_) => break,
            // Браузерные пинги tungstenite отвечает сам; на всякий случай:
            Message::Ping(p) => {
                let _ = p;
                continue;
            }
        };
        let Some(msg) = popproto_core::decode::<ClientMsg>(&bytes) else {
            continue;
        };
        match dispatch(
            &state,
            &record,
            msg,
            &direct_tx,
            &mut room_id,
            &mut forwarder,
        )
        .await
        {
            Ok(()) => {}
            Err((code, text)) => {
                if direct_tx.send(ServerMsg::Error { code, text }).is_err() {
                    break;
                }
            }
        }
    }

    // Разрыв соединения: слот держится открытым на окно реконнекта (бот подхватывает).
    if let Some(rid) = room_id {
        state.lobby.player_disconnected(record.player_id, rid).await;
    }
    if let Some(f) = forwarder.take() {
        f.abort();
    }
    drop(direct_tx);
    let _ = writer.await;
    state
        .ws_clients
        .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
}

type DispatchErr = (ErrCode, String);

async fn dispatch(
    state: &AppState,
    record: &PlayerRecord,
    msg: ClientMsg,
    direct_tx: &mpsc::UnboundedSender<ServerMsg>,
    room_id: &mut Option<u32>,
    forwarder: &mut Option<tokio::task::JoinHandle<()>>,
) -> Result<(), DispatchErr> {
    match msg {
        ClientMsg::Ping(n) => {
            direct_tx
                .send(ServerMsg::Pong(n))
                .map_err(|_| (ErrCode::InvalidInput, "соединение закрыто".into()))?;
        }
        ClientMsg::RequestRoomList => {
            let list = state.lobby.room_list().await;
            direct_tx.send(ServerMsg::RoomList(list)).map_err(choked)?;
        }
        ClientMsg::CreateRoom {
            name,
            password,
            mode,
            settings,
        } => {
            let op: BoxFuture<'_, Result<u32, crate::lobby::LobbyError>> = Box::pin(async {
                state
                    .lobby
                    .create_room(record, name, password, mode, settings)
                    .await
            });
            attach(state, record, direct_tx, room_id, forwarder, op).await?;
        }
        ClientMsg::JoinRoom { code, password } => {
            let op: BoxFuture<'_, Result<u32, crate::lobby::LobbyError>> = Box::pin(async {
                state
                    .lobby
                    .join_room(record, &code, password.as_deref())
                    .await
                    .map(|j| j.room_id)
            });
            attach(state, record, direct_tx, room_id, forwarder, op).await?;
        }
        ClientMsg::LeaveRoom => {
            if let Some(rid) = room_id.take() {
                state.lobby.leave_room(record.player_id, rid).await;
            }
            if let Some(f) = forwarder.take() {
                f.abort();
            }
            let list = state.lobby.room_list().await;
            direct_tx.send(ServerMsg::RoomList(list)).map_err(choked)?;
        }
        ClientMsg::SetReady(ready) => {
            let rid = room_id.ok_or_else(|| (ErrCode::NotAllowed, "вы не в комнате".into()))?;
            state
                .lobby
                .set_ready(record.player_id, rid, ready)
                .await
                .map_err(lobby)?;
        }
        ClientMsg::StartMatch => {
            let rid = room_id.ok_or_else(|| (ErrCode::NotAllowed, "вы не в комнате".into()))?;
            state
                .lobby
                .start_match(record.player_id, rid)
                .await
                .map_err(lobby)?;
            // Игровой цикл матча подключается на шагах 7–8.
        }
        ClientMsg::Kick { player_id } => {
            let rid = room_id.ok_or_else(|| (ErrCode::NotAllowed, "вы не в комнате".into()))?;
            state
                .lobby
                .kick(record.player_id, rid, player_id)
                .await
                .map_err(lobby)?;
        }
        ClientMsg::Chat { text } => {
            let rid = room_id.ok_or_else(|| (ErrCode::NotAllowed, "вы не в комнате".into()))?;
            let in_lobby_phase = state
                .lobby
                .room_state(rid)
                .await
                .map(|s| s.phase == popproto_core::RoomPhase::Lobby)
                .unwrap_or(false);
            if !in_lobby_phase {
                return Err((ErrCode::NotAllowed, "в бою только колесо эмоций".into()));
            }
            let text = text.trim().chars().take(120).collect::<String>();
            if text.is_empty() {
                return Err((ErrCode::InvalidInput, "пустое сообщение".into()));
            }
            broadcast_to(
                state,
                rid,
                ServerMsg::Chat {
                    player_id: record.player_id,
                    text,
                },
            )
            .await?;
        }
        ClientMsg::Emote { emote } => {
            if emote > 9 {
                return Err((ErrCode::InvalidInput, "нет такой эмоции".into()));
            }
            let rid = room_id.ok_or_else(|| (ErrCode::NotAllowed, "вы не в комнате".into()))?;
            broadcast_to(
                state,
                rid,
                ServerMsg::Emote {
                    player_id: record.player_id,
                    emote,
                },
            )
            .await?;
        }
        // Управление матчем: команды уходят в игровой цикл комнаты.
        ClientMsg::PaddleX(x) => {
            if let Some(rid) = room_id {
                state
                    .lobby
                    .game_cmd(
                        *rid,
                        GameCmd::PaddleX {
                            player_id: record.player_id,
                            x,
                        },
                    )
                    .await;
            }
        }
        ClientMsg::Action => {
            if let Some(rid) = room_id {
                state
                    .lobby
                    .game_cmd(
                        *rid,
                        GameCmd::Action {
                            player_id: record.player_id,
                        },
                    )
                    .await;
            }
        }
        ClientMsg::AddBot { level } => {
            let rid = room_id.ok_or_else(|| (ErrCode::NotAllowed, "вы не в комнате".into()))?;
            state
                .lobby
                .add_bot(record.player_id, rid, level)
                .await
                .map_err(lobby)?;
        }
        ClientMsg::SetColor { color } => {
            let rid = room_id.ok_or_else(|| (ErrCode::NotAllowed, "вы не в комнате".into()))?;
            state
                .lobby
                .set_color(record.player_id, rid, color)
                .await
                .map_err(lobby)?
        }
        ClientMsg::SetUltimate { ult } => {
            let rid = room_id.ok_or_else(|| (ErrCode::NotAllowed, "вы не в комнате".into()))?;
            state
                .lobby
                .set_ult(record.player_id, rid, ult)
                .await
                .map_err(lobby)?
        }
        ClientMsg::BuySkin { skin } => {
            state
                .lobby
                .buy_skin(record.player_id, skin)
                .await
                .map_err(lobby)?;
        }
        ClientMsg::EquipSkin { skin } => {
            state.lobby.equip_skin(record.player_id, skin).await;
        }
        ClientMsg::ClaimQuest { index } => {
            let reward = state
                .lobby
                .claim_quest(record.player_id, index)
                .await
                .map_err(lobby)?;
            direct_tx
                .send(ServerMsg::Error {
                    code: ErrCode::InvalidInput,
                    text: format!("🎁 +{reward} XP за квест!"),
                })
                .map_err(choked)?;
        }
        ClientMsg::StartTournament => {
            let rid = room_id.ok_or_else(|| (ErrCode::NotAllowed, "вы не в комнате".into()))?;
            state
                .lobby
                .start_tournament(record.player_id, rid)
                .await
                .map_err(lobby)?;
        }
        ClientMsg::GetReplays => {
            direct_tx
                .send(ServerMsg::ReplayList {
                    items: state.lobby.replay_list(),
                })
                .map_err(choked)?;
        }
        ClientMsg::GetReplay { id } => {
            if let Some((meta, frames)) = state.lobby.replay_data(id) {
                direct_tx
                    .send(ServerMsg::ReplayData { meta, frames })
                    .map_err(choked)?;
            } else {
                direct_tx
                    .send(ServerMsg::Error {
                        code: ErrCode::RoomNotFound,
                        text: "реплей не найден".into(),
                    })
                    .map_err(choked)?;
            }
        }
        ClientMsg::ClanCreate { tag, name } => {
            let tag = tag.trim().to_uppercase();
            if tag.len() < 2 || tag.len() > 4 || !tag.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err((
                    ErrCode::BadName,
                    "тег клана: 2-4 латинских букв/цифр".into(),
                ));
            }
            let name = name.trim().to_string();
            if name.is_empty() || name.chars().count() > 20 {
                return Err((ErrCode::BadName, "название клана: 1-20 символов".into()));
            }
            state
                .lobby
                .clan_create(record.player_id, &record.name, &tag, &name)
                .await
                .map_err(|e| (ErrCode::InvalidInput, e))?;
            direct_tx
                .send(ServerMsg::Error {
                    code: ErrCode::InvalidInput,
                    text: format!("🏆 Клан [{tag}] {name} создан!"),
                })
                .map_err(choked)?;
        }
        ClientMsg::ClanJoin { tag } => {
            let tag = tag.trim().to_uppercase();
            state
                .lobby
                .clan_join(record.player_id, &tag)
                .await
                .map_err(|e| (ErrCode::InvalidInput, e))?;
            direct_tx
                .send(ServerMsg::Error {
                    code: ErrCode::InvalidInput,
                    text: format!("🏆 Вы вступили в клан [{tag}]"),
                })
                .map_err(choked)?;
        }
        ClientMsg::ClanLeave => {
            state
                .lobby
                .clan_leave(record.player_id)
                .await
                .map_err(|e| (ErrCode::InvalidInput, e))?;
            direct_tx
                .send(ServerMsg::Error {
                    code: ErrCode::InvalidInput,
                    text: "🏆 Вы покинули клан".into(),
                })
                .map_err(choked)?;
        }
        ClientMsg::PublishMap { name, rows } => {
            let name = name.trim().to_string();
            if name.is_empty() || name.chars().count() > 24 {
                return Err((ErrCode::BadName, "имя карты: 1-24 символов".into()));
            }
            let mut clean: Vec<String> = rows
                .into_iter()
                .take(8)
                .map(|r| {
                    let mut r: String = r
                        .chars()
                        .take(10)
                        .filter(|c| matches!(c, '1'..='3' | 'x' | '.' | 'e' | 't' | 'g' | 'm'))
                        .collect();
                    while r.chars().count() < 10 {
                        r.push('.');
                    }
                    r
                })
                .filter(|r| r.chars().any(|c| c != '.'))
                .collect();
            if clean.is_empty() {
                clean.push("..........".into());
            }
            state
                .lobby
                .publish_map(&record.name, &name, clean)
                .await
                .map_err(lobby)?;
        }
        ClientMsg::RequestMaps => {} // список карт клиент берёт по HTTP /api/maps
        ClientMsg::UseUltimate => {
            if let Some(rid) = room_id {
                state
                    .lobby
                    .game_cmd(
                        *rid,
                        GameCmd::Ultimate {
                            player_id: record.player_id,
                        },
                    )
                    .await;
            }
        }
        ClientMsg::RemoveBot { player_id } => {
            let rid = room_id.ok_or_else(|| (ErrCode::NotAllowed, "вы не в комнате".into()))?;
            state
                .lobby
                .remove_bot(record.player_id, rid, player_id)
                .await
                .map_err(lobby)?;
        }
        ClientMsg::Hello { .. } => {}
    }
    Ok(())
}

/// Общая часть Create/Join: выполнить операцию, подписаться на комнату,
/// переслать новичку её состояние.
async fn attach(
    state: &AppState,
    record: &PlayerRecord,
    direct_tx: &mpsc::UnboundedSender<ServerMsg>,
    room_id: &mut Option<u32>,
    forwarder: &mut Option<tokio::task::JoinHandle<()>>,
    op: BoxFuture<'_, Result<u32, crate::lobby::LobbyError>>,
) -> Result<(), DispatchErr> {
    // Уже в другой комнате — сначала выходим.
    if let Some(old) = room_id.take() {
        state.lobby.leave_room(record.player_id, old).await;
        if let Some(f) = forwarder.take() {
            f.abort();
        }
    }
    let new_id = op.await.map_err(lobby)?;
    let mut rx = state
        .lobby
        .subscribe(new_id)
        .await
        .ok_or_else(|| (ErrCode::RoomNotFound, "комната исчезла".into()))?;
    let tx = direct_tx.clone();
    *forwarder = Some(tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if tx.send(msg).is_err() {
                break;
            }
        }
    }));
    *room_id = Some(new_id);
    let state_msg = state.lobby.room_state(new_id).await;
    if let Some(s) = state_msg {
        direct_tx
            .send(ServerMsg::RoomState(Box::new(s)))
            .map_err(choked)?;
    }
    Ok(())
}

async fn broadcast_to(state: &AppState, room_id: u32, msg: ServerMsg) -> Result<(), DispatchErr> {
    state.lobby.broadcast_raw(room_id, msg).await.map_err(lobby)
}

fn choked(_: mpsc::error::SendError<ServerMsg>) -> DispatchErr {
    (ErrCode::InvalidInput, "соединение закрыто".into())
}

fn lobby(e: crate::lobby::LobbyError) -> DispatchErr {
    match e {
        crate::lobby::LobbyError::Code(c) => (c, "операция отклонена".into()),
        crate::lobby::LobbyError::Backend(s) => (ErrCode::InvalidInput, s),
    }
}

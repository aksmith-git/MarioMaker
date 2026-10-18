//! Online play in the browser: a [`Link`] over a WebSocket to the course server's relay (the
//! server's `ws` module), joining the same rooms as native players (`mmk_netplay::relay`).
//! Messages are the relay's binary messages; replies are queued by the socket's `onmessage` and
//! drained each tick by `poll`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use mmk_netplay::relay::{KEEPALIVE_SENDS, data_bytes, join_bytes, parse_data, parse_welcome};
use mmk_netplay::{Link, Packet};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{BinaryType, MessageEvent, WebSocket};

pub struct WsLink {
    ws: WebSocket,
    inbox: Rc<RefCell<VecDeque<Vec<u8>>>>,
    join: Vec<u8>,
    sends: u32,
    /// Members in the room (from the last welcome).
    pub members: usize,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_open: Closure<dyn FnMut()>,
}

impl WsLink {
    /// Connect to the relay at `url` (`ws://host:port` or `wss://…`) and join `room` as `slot`.
    pub fn connect(url: &str, room: &str, slot: u8) -> Result<Self, JsValue> {
        if room.is_empty() || room.len() > mmk_netplay::relay::MAX_ROOM_NAME {
            return Err(JsValue::from_str("room name: 1–16 bytes"));
        }
        let ws = WebSocket::new(url)?;
        ws.set_binary_type(BinaryType::Arraybuffer);
        let inbox: Rc<RefCell<VecDeque<Vec<u8>>>> = Rc::default();
        let q = inbox.clone();
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |e: MessageEvent| {
            if let Ok(buf) = e.data().dyn_into::<js_sys::ArrayBuffer>() {
                q.borrow_mut()
                    .push_back(js_sys::Uint8Array::new(&buf).to_vec());
            }
        });
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        let join = join_bytes(room, slot);
        let (w, j) = (ws.clone(), join.clone());
        let on_open = Closure::<dyn FnMut()>::new(move || {
            let _ = w.send_with_u8_array(&j);
        });
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        Ok(Self {
            ws,
            inbox,
            join,
            sends: 0,
            members: 0,
            _on_message: on_message,
            _on_open: on_open,
        })
    }
}

impl Link for WsLink {
    fn send(&mut self, p: &Packet) {
        if self.ws.ready_state() != WebSocket::OPEN {
            return;
        }
        if self.sends.is_multiple_of(KEEPALIVE_SENDS) {
            let _ = self.ws.send_with_u8_array(&self.join);
        }
        self.sends = self.sends.wrapping_add(1);
        let _ = self.ws.send_with_u8_array(&data_bytes(p));
    }

    fn poll(&mut self) -> Vec<Packet> {
        let mut out = Vec::new();
        for m in self.inbox.borrow_mut().drain(..) {
            if let Some(p) = parse_data(&m) {
                out.push(p);
            } else if let Some(w) = parse_welcome(&m) {
                self.members = w.len();
            }
        }
        out
    }
}

impl Drop for WsLink {
    fn drop(&mut self) {
        self.ws.set_onmessage(None);
        self.ws.set_onopen(None);
        let _ = self.ws.close();
    }
}

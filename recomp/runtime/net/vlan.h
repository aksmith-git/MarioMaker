// A virtual LAN for a game's own local multiplayer (runtime/services/bsd.c): the game's UDP sockets live on
// 10.13.37.<slot + 1>/24 and every datagram goes through a room on our course server's relay (core/crates/netplay
// relay.rs: MMKJ join / keepalive, MMKD data, forwarded to the room's other members), so recomp players anywhere play
// together as if on one LAN. Nothing reaches any other host: a socket can only talk to the room.
//
// On when LARECOMP_LAN_RELAY=<host>:<port> (the server's HTTP port: the relay listens on the same UDP port) and
// LARECOMP_LAN_ROOM=<name> are set (or a game supplies them: game_lan_config, runtime/game_hooks.c);
// LARECOMP_LAN_SLOT=0..3 picks the player's address (default 0).
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define VLAN_NET 0x0A0D2500u   // 10.13.37.0 (host order)
#define VLAN_MASK 0xFFFFFF00u
#define VLAN_MAX_PAYLOAD 1400  // per datagram (relay messages up to 1500 bytes)

bool vlan_active(void);   // configured (starts the relay link on first use)
uint32_t vlan_ip(void);   // this player's address (host order), 0 when off
// Send one datagram from src_port to dst_ip:dst_port (host order; dst_ip may be the subnet broadcast or
// 255.255.255.255). Returns the length sent or -1.
int vlan_send(uint16_t src_port, uint32_t dst_ip, uint16_t dst_port, const void *data, size_t len);
// Datagrams for this player are handed to deliver(): (src_ip, src_port, dst_port, data, len), on the relay thread.
void vlan_set_receiver(void (*deliver)(uint32_t src_ip, uint16_t src_port, uint16_t dst_port, const uint8_t *data,
                                       size_t len));

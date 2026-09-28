---
title: MQTT
description: Connect to an MQTT broker (3.1.1 or 5), subscribe to topic filters and publish messages with QoS 0, 1 or 2.
sidebar:
  order: 7
---

An MQTT client request connects to a broker, subscribes to topic filters and publishes messages. Incoming messages appear live with their topic, QoS and retain flag.

## Create an MQTT client

In the **Collection** sidebar, open the **New** menu (**+**) and choose **New MQTT client**. The request has these tabs: **Connection**, **Subscriptions**, **Auth**, **Settings** and **Docs**.

## Broker address

| Address | Connection | Default port |
|---|---|---|
| `mqtt://host[:port]` (or `tcp://`) | Plain TCP | 1883 |
| `mqtts://host[:port]` (or `ssl://`, `tls://`) | TLS | 8883 |
| `host[:port]` | No scheme means `mqtt://` | 1883 |

- Connections are direct: the HTTP proxy is not used.
- `mqtts://` is verified with this computer's trusted certificates and **Settings → Certificates** (a client certificate set there is used for mutual TLS). **Verify TLS certificates** in the **Settings** tab turns the check off. See [TLS & certificates](../../requests/tls-and-certificates/).
- MQTT over WebSocket (`ws://`, `wss://`) is not supported: the error says **MQTT over WebSocket is not supported yet: use mqtt:// or mqtts://**.

## Connection settings

The **Connection** tab:

| Setting | Default | Description |
|---|---|---|
| **Client ID** | empty | Empty: a new random ID (`zorvik-` followed by 8 hex digits) on every connect. Brokers drop an older connection with the same ID. `{{variables}}` allowed. |
| **MQTT version** | 3.1.1 | `3.1.1` or `5`. |
| **Keep alive (seconds)** | 30 | A ping goes out when nothing else was sent for this long; the connection closes when the broker stops answering. `0` turns keep alive off. An MQTT 5 broker may set another value; the log then says so. |
| **Clean session** | on | Off: the broker keeps your subscriptions and queued QoS 1 and 2 messages while you are away. With MQTT 5, Zorvik asks the broker to keep the session for up to an hour. Use a fixed Client ID for that. |

Changes apply the next time you connect.

### User name and password

The user name and password come from **Basic** auth in the **Auth** tab (set on the request, or inherited from its folder or the workspace; variables allowed). Other auth types are not used by MQTT. MQTT 3.1.1 needs a user name when a password is set. The client ID, user name and password must each be shorter than 64 KB.

## Connect

Press **Connect**. Zorvik opens the connection, sends `CONNECT` and waits for the broker's `CONNACK` within the connect timeout from Settings. When the broker refuses, the error says why, for example **The broker refused the connection: not authorized**.

The first log line after connecting shows the client ID (the one the broker assigned, if it did), **resumed the previous session** when the broker kept one, and a changed keep alive.

## Subscriptions

The **Subscriptions** tab lists topic filters with a QoS (0, 1 or 2) and an on/off checkbox. Type into the last, empty row to add one.

- Before connecting, the enabled filters are subscribed right after the connection opens.
- While connected, changes apply at once: turning a row on or off subscribes or unsubscribes, changing the QoS subscribes again, editing a topic (when you press <kbd>Enter</kbd> or leave the field) unsubscribes the old filter and subscribes the new one, and removing a row unsubscribes.
- A dot on each row shows whether it is subscribed on this connection, with the QoS the broker granted.
- The log shows **Subscribed to sensors/+/temperature (QoS 1)**, or **(QoS 0; asked for 1)** when the broker granted less, and **Subscription to … failed: …** when it refused.

Filters use MQTT wildcards: `+` matches one level and `#` matches the rest; both must be whole levels and `#` must come last. Examples: `sensors/+/temperature`, `devices/#`. `{{variables}}` are allowed.

## Publish

The composer at the bottom publishes messages:

| Control | Description |
|---|---|
| **Topic** | The topic to publish to, for example `devices/42/state`. No wildcards. `{{variables}}` allowed. |
| **QoS** | 0, 1 or 2. |
| **Retain** | The broker keeps the last retained message of the topic and sends it to new subscribers. |
| **Text** / **JSON** / **Binary (hex)** | The payload: text (with `{{variables}}` replaced) or hex bytes. |

Press **Send** or <kbd>Mod</kbd>+<kbd>Enter</kbd>. Zorvik checks what the broker announced before sending: the highest QoS it accepts, whether it supports retained messages, and how many QoS 1 and 2 messages may wait for its acknowledgement at once. The acknowledgement flows (`PUBACK`, or `PUBREC`/`PUBREL`/`PUBCOMP` for QoS 2) are handled for you, in both directions. Nothing is stored on disk.

## The message log

Incoming and outgoing messages show their topic as a badge and, in the details, the QoS and flags, for example **QoS 1 · retained** or **QoS 2 · duplicate**. Payloads are shown as text when they are valid UTF-8, otherwise as hex. Subscriptions, errors and the broker disconnecting (**The broker disconnected: …**) appear as notes.

Packets up to 16 MB are accepted and sent.

## Saved format

```yaml title="requests/Sensors.yaml"
name: Sensors
kind: mqtt
url: mqtts://broker.example.com:8883
auth:
  type: basic
  username: "{{mqttUser}}"
  password: "{{mqttPassword}}"
mqtt:
  clientId: zorvik-dev
  version: v5
  cleanSession: false
  keepAliveSecs: 30
  subscriptions:
    - topic: sensors/+/temperature
      qos: 1
    - topic: devices/#
      qos: 0
      enabled: false
  topic: devices/42/commands
  qos: 1
  retain: false
body:
  type: json
  text: '{"command": "reboot"}'
```

| Field | Default | Description |
|---|---|---|
| `mqtt.clientId` | empty | Empty: random per connection. |
| `mqtt.version` | `v311` | `v311` or `v5`. |
| `mqtt.cleanSession` | `true` | Clean session / clean start. |
| `mqtt.keepAliveSecs` | `30` | Keep alive in seconds (`0` = off). |
| `mqtt.subscriptions` | none | `topic`, `qos` (0–2) and `enabled` (default `true`) per filter. |
| `mqtt.topic` | empty | Topic the composer publishes to. |
| `mqtt.qos` | `0` | QoS of published messages. |
| `mqtt.retain` | `false` | Retain flag of published messages. |
| `body.text` | empty | The composer payload. |

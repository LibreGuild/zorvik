---
id: mqtt-pubsub
title: "MQTT: publish and subscribe"
summary: Devices publish messages to named topics on a broker, and every client that subscribed to a matching topic receives them.
minutes: 6
lab:
  title: Talk to a tiny broker
  goal: Connect to a broker, subscribe to a sensor's topic, read its reading and switch on a light.
  minutes: 8
  servers:
    broker:
      name: Tiny MQTT broker
      kind: tcp
      socket:
        mode: rules
        encoding: hex
        rules:
          # SUBSCRIBE (packet type, length, packet id, filter length) to a filter that
          # matches lab/sensors/temp, then the QoS byte: SUBACK, then one PUBLISH on
          # lab/sensors/temp with {"room":"lab","celsius":21.5} (QoS 0).
          - match: regex
            pattern: '(?s-u)^\x82.{5}(#|(lab|\+)/(#|(sensors|\+)/(#|temp|\+)))[\x00-\x02]$'
            reply: 90 03 00 01 00 30 2f 00 10 6c 61 62 2f 73 65 6e 73 6f 72 73 2f 74 65 6d 70 7b 22 72 6f 6f 6d 22 3a 22 6c 61 62 22 2c 22 63 65 6c 73 69 75 73 22 3a 32 31 2e 35 7d
          # Any other SUBSCRIBE: SUBACK only.
          - match: regex
            pattern: '(?-u)^\x82'
            reply: 90 03 00 01 00
          # CONNECT: CONNACK, connection accepted (MQTT 3.1.1).
          - match: regex
            pattern: '^\x10'
            reply: 20 02 00 00
          # PINGREQ: PINGRESP.
          - match: regex
            pattern: '(?-u)^\xc0'
            reply: d0 00
  steps:
    - text: |
        Click **+** at the end of the tab bar and choose **MQTT client**. Set the URL to `mqtt://{{broker_host}}` and press **Connect**. Leave **MQTT version** on 3.1.1 in the **Connection** tab.
      hints:
        - "MQTT addresses start with mqtt:// (or mqtts:// with TLS), followed by the broker's host and port."
        - "The + menu of the tab bar lists MQTT client. {{broker_host}} holds 127.0.0.1 and the broker's port."
        - "New MQTT client, URL mqtt://{{broker_host}}, press Connect. The log shows Connected and your client ID."
      check:
        call: { method: socket.connect, params: { request: { kind: mqtt } }, ok: true }
      solution:
        - call: { method: socket.connect, params: { connId: lab-mqtt, request: { name: Lab broker, kind: mqtt, url: "mqtt://{{broker_host}}" } } }
    - text: |
        Open the **Subscriptions** tab, type the topic filter `lab/sensors/#` into the empty row (keep QoS 0), then click outside the field. While you're connected, Zorvik subscribes as soon as you leave the field.
      hints:
        - "The # wildcard matches everything below lab/sensors/, so you'll get every sensor's messages."
        - "In the Subscriptions tab, type into the empty row that says Add a topic filter…"
        - "Type lab/sensors/# in the Topic filter column, keep QoS 0 and press Tab. The log then says Subscribed to lab/sensors/# (QoS 0)."
      check:
        any:
          - call: { method: socket.connect, params: { request: { kind: mqtt, mqtt: { subscriptions: [{ topic: "lab/*" }] } } }, ok: true }
          - call: { method: socket.send, params: { message: { type: subscribe, topic: "lab/*" } }, ok: true }
      solution:
        - call: { method: socket.connect, params: { connId: lab-mqtt, request: { name: Lab broker, kind: mqtt, url: "mqtt://{{broker_host}}", mqtt: { subscriptions: [{ topic: "lab/sensors/#", qos: 0 }] } } } }
        - wait: 300
    - text: A reading arrived on the topic `lab/sensors/temp`. What temperature, in °C, did the sensor report?
      hints:
        - "Received messages have a blue arrow and show their topic as a badge."
        - "The message is JSON. Look for the celsius field."
        - "The sensor sent {\"room\":\"lab\",\"celsius\":21.5}, so the answer is 21.5."
      check:
        answer: ["21.5", "21.5*", "21,5"]
      solution:
        - answer: "21.5"
    - text: |
        Time to publish. In the composer at the bottom, set **Topic** to `lab/lights/kitchen`, type `on` as the message and press **Send**.
      hints:
        - "Publishing needs a topic and a message. The QoS selector and Retain box can stay as they are."
        - "The composer has a Topic field next to the message box. Leave QoS on 0."
        - "Topic lab/lights/kitchen, message on, then Send. The broker's Traffic shows your PUBLISH arrive."
      check:
        message: { server: broker, direction: in, text: "*lab/lights/kitchen*on*" }
      solution:
        - call: { method: socket.connect, params: { connId: lab-mqtt, request: { name: Lab broker, kind: mqtt, url: "mqtt://{{broker_host}}" } } }
        - call: { method: socket.send, params: { connId: lab-mqtt, message: { type: publish, topic: lab/lights/kitchen, text: "on", qos: 0 } } }
        - wait: 300
        - call: { method: socket.close, params: { connId: lab-mqtt } }
quiz:
  - question: A thermometer publishes to `home/kitchen/temp`. Which subscription does NOT receive it?
    options:
      - home/#
      - home/+/temp
      - home/kitchen
    answer: 2
    explain: A filter without wildcards must match the topic exactly. home/# matches everything below home, and + matches one level.
  - question: Who delivers a published message to the subscribers?
    options:
      - The broker, which every client is connected to
      - The publisher, which connects to every subscriber itself
      - Nobody; subscribers poll the publisher
    answer: 0
    explain: Publishers and subscribers never talk directly. They only know the broker and the topic names.
  - question: Your message must arrive, and a duplicate now and then is harmless. Which QoS fits?
    options:
      - QoS 0, at most once
      - QoS 1, at least once
      - QoS 2, exactly once
    answer: 1
    explain: QoS 1 repeats a message until the broker confirms it, so it may arrive twice. QoS 2 avoids duplicates but costs more round trips.
---

**MQTT** is a small, lightweight messaging protocol designed for devices with little power and shaky networks: sensors, smart plugs, cars, factory machines. Phone apps use it for chat and notifications too. It runs over TCP, usually on port `1883` (or `8883` with TLS: `mqtts://`).

## Publish and subscribe

MQTT clients never talk to each other directly. They all connect to one server called the **broker**:

- A client **publishes** a message to a **topic**, a name such as `home/kitchen/temp`.
- A client **subscribes** to the topics it cares about.
- The broker passes every message on to each client whose subscription matches.

```sequence
participants: Sensor, Broker, App
App -> Broker: SUBSCRIBE lab/sensors/#
Broker --> App: SUBACK (subscribed)
Sensor -> Broker: PUBLISH lab/sensors/temp 21.5
Broker --> App: PUBLISH lab/sensors/temp 21.5
```

The sensor doesn't know who listens, and the app doesn't know which device sent the reading. Add a second app, or a hundred sensors, and nothing else has to change.

> [!note] Think of it like…
> A noticeboard in a busy office. People pin notes under headings ("Kitchen", "Parking"), and everyone who asked to follow a heading gets a copy. Nobody needs to know who else reads it.

## Topics and wildcards

Topics are levels separated by `/`. Subscriptions may use two wildcards:

| Filter | Matches | Doesn't match |
|---|---|---|
| `home/kitchen/temp` | exactly that topic | `home/hall/temp` |
| `home/+/temp` | one level: `home/kitchen/temp`, `home/hall/temp` | `home/kitchen/fridge/temp` |
| `home/#` | everything below `home` | `office/temp` |

## Quality of service (QoS)

Each message is sent with a **QoS** level, a promise about delivery:

- **QoS 0, at most once:** send and forget. Fast, but a message can be lost.
- **QoS 1, at least once:** repeated until the receiver confirms it, so it may arrive twice.
- **QoS 2, exactly once:** a four-step handshake, the slowest and safest.

Two more terms you'll meet: a **retained** message is kept by the broker and handed to anyone who subscribes later (handy for "the light is on"). **Keep alive** makes the client send a small ping when it has been quiet, so both sides notice a dead connection.

## In Zorvik

An **MQTT client** request has the tabs **Connection** (client ID, MQTT version, keep alive, clean session), **Subscriptions** (topic filters and their QoS) and **Auth** (user name and password). After **Connect**, the log shows every message with its topic as a badge, and the composer at the bottom publishes: a **Topic**, a QoS, **Retain**, your message, **Send**.

> [!warning] The lab's broker is a stand-in
> Real brokers such as Mosquitto, EMQX or HiveMQ route messages between many clients. The lab's **Lab · Tiny MQTT broker** is a small Zorvik TCP server with reply rules that knows just enough MQTT to accept you, confirm a subscription and send one sensor reading. Open it under **Servers** to see MQTT's binary packets in its Traffic.

**You'll use this when…** you test a smart-home app or a fleet of devices: you subscribe to their topics to watch what they report, and publish commands yourself to see how the devices and apps react.

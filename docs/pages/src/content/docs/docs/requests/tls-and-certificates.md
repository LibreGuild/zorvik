---
title: TLS & certificates
description: How Zorvik verifies server certificates with the OS trust store, and how to add a private CA, use client certificates for mutual TLS, turn verification off and read TLS details.
sidebar:
  order: 5
---

Every `https://` request is encrypted with TLS. Zorvik checks the server's certificate the way your browser does, and shows you what was negotiated.

## Certificate verification

By default Zorvik verifies that the server's certificate chain leads to a trusted root and that the certificate matches the host name.

Trusted roots come from your **operating system's trust store**:

| System | Trust store |
|---|---|
| Windows | The Windows certificate store |
| macOS | The keychain |
| Linux | The system's CA certificates |

So certificate authorities your company installs on your computer, for example for TLS inspection by a corporate proxy, are trusted without any setup in Zorvik.

Zorvik speaks TLS 1.2 and TLS 1.3. Older versions (TLS 1.0, TLS 1.1 and SSL) are not supported. HTTP/3 always uses TLS 1.3.

## Add a private CA

For servers whose certificates come from a private or self-signed certificate authority that isn't in your OS trust store:

1. Open **Settings → Certificates**.
2. Under **Extra CA certificate**, choose **Browse…** and pick the CA's certificate, or type its path.
3. Choose **Save**.

| Setting | Detail |
|---|---|
| Format | PEM (`-----BEGIN CERTIFICATE-----`). One file can hold several certificates. |
| Effect | Trusted **in addition to** the OS trust store, which stays in use |
| Scope | Every request, in every workspace |
| Changes | Zorvik notices when the file changes on disk |

To trust the CA everywhere on your computer (browsers, curl, other tools) install it in your OS trust store instead; Zorvik then trusts it with no extra setting.

:::tip[Converting to PEM]
A certificate in DER format (often `.cer` or `.der`) can be converted with OpenSSL:

```bash
openssl x509 -inform der -in ca.cer -out ca.pem
```
:::

## Client certificates (mutual TLS)

Some servers ask the client for a certificate too. To send one:

1. Open **Settings → Certificates**.
2. **Client certificate**: the PEM certificate. The file may also contain intermediate certificates after yours.
3. **Client key**: the PEM private key for that certificate.
4. Choose **Save**.

| Rule | Detail |
|---|---|
| Both or neither | Set both files. With only one, requests fail with *Client certificate and client key must both be set for mutual TLS*. |
| Key format | An unencrypted PEM private key: PKCS#8 (`BEGIN PRIVATE KEY`), PKCS#1 RSA (`BEGIN RSA PRIVATE KEY`) or SEC1 EC (`BEGIN EC PRIVATE KEY`). Keys protected by a passphrase are not supported. |
| Scope | The certificate is offered to **every** server that asks for a client certificate, in every workspace. There is no per-host setting. |

**Clear** next to a path removes it.

:::tip[From a .p12 or .pfx file]
Zorvik reads PEM files only. To split a PKCS#12 bundle into a certificate and a key:

```bash
openssl pkcs12 -in client.p12 -clcerts -nokeys -out client.crt
openssl pkcs12 -in client.p12 -nocerts -nodes -out client.key
```

`-nodes` writes the key without a passphrase, so keep `client.key` somewhere private.
:::

## Turning verification off

For a test server with a self-signed certificate that you trust, you can skip verification:

- **For one request**: the request's **Settings** tab → **Verify TLS certificates** → **Off**. **App default** follows the global setting; **On** forces verification.
- **For every request**: **Settings → Requests → Verify TLS certificates**.

With verification off, Zorvik accepts any certificate for any host name; the connection is still encrypted. The extra CA is not needed then, and client certificates are still sent.

:::danger
Without verification, anyone between you and the server can read and change the traffic. Turn it off only for servers you control, and prefer adding the CA instead.
:::

```yaml title="In the request file"
settings:
  verifyTls: false
```

## What applies where

The certificate settings apply to every TLS connection Zorvik makes as a client:

- HTTPS requests, including GraphQL and HTTP/3
- WebSocket (`wss://`), Socket.IO and GraphQL subscriptions over TLS, event streams, gRPC (`grpcs://`), TCP over TLS, MQTT over TLS (`mqtts://`), DNS over TLS and DNS over HTTPS
- OAuth 2.0 token requests and OpenAPI imports from a URL
- Load tests

WebSocket, Socket.IO, event stream, gRPC, TCP and MQTT requests have the same **Verify TLS certificates** setting in their **Settings** tab.

:::note[Command line]
The `zorvik` command line doesn't read the app's settings, so the extra CA and the client certificate don't apply there. It trusts the OS trust store, and `-k` (`--insecure`) skips verification. See [Command line](../../cli/overview/).
:::

## TLS details of a response

A lock next to the status means the response came over TLS. The response's **Info** tab lists, under **Security**:

| Field | Example |
|---|---|
| **TLS** | `TLS 1.3` |
| **Cipher** | `TLS13_AES_128_GCM_SHA256` |
| **ALPN** | `h2`, `http/1.1` or `h3` (the negotiated application protocol) |
| **Subject** | The server certificate's subject |
| **Issuer** | Who issued it |
| **Valid** | From and until dates |
| **Names** | The subject alternative names (host names and IP addresses) |
| **Serial** | The serial number |

A plain `http://` response shows **TLS: Not encrypted**. The **Timing** tab shows how long the TLS handshake took.

For a full certificate chain, expiry warnings and the protocol versions and ciphers a server accepts, use the **TLS inspector** in the **Tools** section of the left rail.

## TLS errors

A failed handshake shows **TLS / certificate error** with the reason and a hint:

| Message contains | Meaning | What to do |
|---|---|---|
| *The certificate is not trusted* (`UnknownIssuer`) | The chain doesn't lead to a trusted root | Add the CA as **Extra CA certificate**, or install it in your OS trust store |
| *The certificate does not match the host name* | The certificate is for other names | Use a host name listed under **Names**, or fix the certificate |
| *The certificate has expired* | Its validity ended | Renew the certificate |

**Open settings** in the error takes you to Settings. See also [Troubleshooting](../../help/troubleshooting/).

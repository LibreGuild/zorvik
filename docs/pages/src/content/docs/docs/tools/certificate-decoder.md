---
title: Certificate decoder
description: See what a PEM certificate, chain, certificate request or key holds, check the order of a chain, and find the certificate a private key belongs to.
sidebar:
  order: 3
---

The **Certificate decoder** shows what a certificate file holds without connecting anywhere: names, issuer, validity, key, fingerprints and usages. It checks the order of a chain and tells you which certificate a private key belongs to, the usual questions when you set up a client certificate, a CA for **Settings → Certificates**, or TLS on a server. Open it from the **Tools** sidebar.

To see the certificates a running server sends, use the [TLS inspector](../network-tools/#tls-inspector) instead.

## Paste or open a file

Paste into **PEM text**, or click **Open file…** to read a file (up to 1 MB). The result updates as you type. **Paste text instead** goes back from a file to the text box, and **✕** clears both.

What it reads:

| Input | Example |
|---|---|
| PEM blocks, any number, in any order | A certificate, a whole chain, a CA bundle, a certificate request and its key |
| A DER file | A binary `.der` or `.cer` certificate or certificate request |
| Base64 without the PEM lines | The base64 body of a certificate, as some tools print it |

Text around the PEM blocks (comments, `subject=` lines from `openssl`) is ignored. PKCS#12 files (`.p12`, `.pfx`) can't be read; export the certificate as PEM first, for example with `openssl pkcs12 -in file.p12 -nokeys`.

:::note[Private keys]
A private key is only used to find the certificate it belongs to. It is never shown, never kept after you close the tab, and never sent anywhere. Keys protected with a password (`ENCRYPTED PRIVATE KEY`) can't be read, so they are only listed.
:::

## What it shows

Each block is a card, numbered in input order:

| Block | Shows |
|---|---|
| **Certificate** (or **Intermediate certificate**, **Root certificate**) | Subject, issuer, validity with days left, the names it covers (SANs), public key, signature algorithm, SHA-256 fingerprint, serial number, **Key usage**, **Extended key usage** (for example **Server authentication**, **Client authentication**) and the X.509 version. **Copy PEM** copies that one certificate, to split a chain or bundle. |
| **Certificate request** (CSR) | Subject, the names it asks for, public key and signature algorithm |
| **Private key** | Its type and size (`EC · P-256 · 256 bits`, `RSA · 2048 bits`) and **Belongs to**: the certificates and requests with the same public key, or **None of the certificates here** |
| **Public key** | Its type and size, and the certificates and requests it belongs to |
| Other blocks | Their label, for example a certificate revocation list (`X509 CRL`), which isn't decoded |

A block that can't be parsed says why, for example **Could not parse the certificate: …**, and the other blocks are still shown.

## Findings

Findings appear above the cards, most severe first:

| Finding | Level | When |
|---|---|---|
| Expired / not valid yet | danger | A certificate is outside its validity period. |
| Key doesn't match | danger | A private key belongs to none of the certificates pasted with it. |
| Expires soon | warning | A certificate expires within 30 days. |
| Weak key | warning | RSA or DSA below 2048 bits, or EC below 256 bits (also for requests). |
| Weak signature | warning | A certificate (other than a root) is signed with SHA-1 or MD5. |
| Chain order | warning | A certificate is not followed by the one that issued it, so the chain is out of order or a certificate is missing. |
| Reverse order | info | The chain starts with the root. Servers send their own certificate first, then each issuer. |

A bundle of root certificates (each self-signed) is not a chain, so it raises no order finding.

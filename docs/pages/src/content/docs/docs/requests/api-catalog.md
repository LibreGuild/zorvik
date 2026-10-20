---
title: API catalog
description: Import well-known APIs (OpenAI, Anthropic, Gemini, Stripe, GitHub, Slack, Google, AWS and many more) in one step, with their requests, sign-in and an environment ready for your key.
sidebar:
  order: 10
---

The API catalog lists well-known APIs you can import in one step: AI and LLM providers, cloud and developer tools, messaging, productivity, payments, commerce, identity and more. Each import brings the API's requests, sets up how they sign in, and makes an environment ready for your key.

## Open the catalog

The catalog is the **API catalog** tab of the [Import dialog](../import-export/#the-import-dialog), its last tab:

- the **Collection** sidebar: **+** → **Import…**, then **API catalog**;
- a folder: right-click → **Import into folder…**, then **API catalog** (the API goes into that folder);
- the start screen: **Browse API catalog** opens it directly.

Search by name, by what the API does or by category, or pick a category: AI, Cloud, Developer tools, Monitoring, Databases & search, Messaging & email, Productivity, Payments, Commerce, CRM & marketing, Identity, Maps & media.

Choose an API to see what it does, how its requests sign in, the format of its description, its license and where it's downloaded from, with a link to the API's own docs.

## Import an API

Choose **Import**. Zorvik downloads the API's description from its publisher and imports it:

- **A folder** named after the API, with a subfolder per group of endpoints.
- **Sign-in** set up on the folder (bearer token, API key, OAuth 2.0, AWS Signature…), using variables.
- **An environment** named after the API, made active, with `baseUrl` and empty [secret variables](../../variables/secrets/) for the credentials. Choose **Add your API key** to fill them in.
- The request to try first opens, such as **List models** or **Get balance**.

Very large APIs (Microsoft Graph, GitHub, Google Compute Engine, AWS EC2…) have thousands of endpoints. For those, Zorvik first lists their groups with how many requests each holds, with the most-used ones ticked. Tick the ones you want and choose **Import**. Import more groups later the same way.

Nothing is downloaded until you choose **Import**, and only from the address the catalog shows. The catalog itself comes with the app and changes with app updates.

## Where the requests come from

| The catalog says | What you get |
|---|---|
| **OpenAPI**, **Swagger** | The publisher's OpenAPI document, imported like [any OpenAPI document](../import-export/#openapi-and-swagger), kept in the workspace for [contract checks](../../testing/openapi-contract-checks/) and **Update from OpenAPI** |
| **Google Discovery** | Google's own description of the API (Gmail, Drive, Calendar, Sheets, YouTube, Gemini, Vertex AI, Cloud Storage, BigQuery…), turned into OpenAPI as it's imported, with Google's OAuth 2.0 (or an API key where the API takes one) |
| **AWS Smithy** | The AWS service model (S3, DynamoDB, Lambda, Bedrock…): requests in the service's own protocol, signed with AWS Signature, with `awsRegion` and your access keys in the environment |
| **Postman collection** | The publisher's Postman collection, imported like [any collection](../import-export/#postman-collections) |
| **OpenAI-compatible** | Providers that speak OpenAI's API (Groq, DeepSeek, OpenRouter, Fireworks, Together, LM Studio…): the OpenAI requests the provider supports, sent to the provider's address |
| **GraphQL** | One GraphQL request with a sample query; the editor completes the rest from the API's schema |
| **Written for Zorvik** | APIs that publish no description of their own (Anthropic, Telegram, Airtable, OpenWeatherMap): requests written from the API's documentation |
| **Link only** | APIs whose terms don't allow their description to be used this way, or that publish nothing Zorvik can import: the catalog says why and links to their docs |

The catalog shows each description's license. A description downloaded from its publisher is used under the publisher's own terms; Zorvik doesn't ship third-party descriptions.

## When a download fails

Publishers move and rename their files from time to time. When a download doesn't work, the catalog says what happened and what to do instead, for example:

> The Stripe API description is no longer at https://… (HTTP 404): the publisher moved or removed it. You can download the Stripe API description from https://docs.stripe.com/api yourself and import the file (Import → File).

| What happened | What the message says |
|---|---|
| No connection, or the publisher's server didn't answer | Check your internet connection and try again |
| The file is gone (`404`, `410`) | The publisher moved or removed it |
| The address now leads to a web page (often after a redirect) | The file has probably moved |
| A redirect led from `https` to plain `http` | The download isn't secure, so it isn't used |
| The publisher refused or is limiting downloads (`401`, `403`, `429`) | Try again later |
| The publisher's server had a problem (`5xx`) | Try again later |
| The file isn't a description Zorvik can read any more | The publisher may have changed it |

Every message ends with the way around it: choose **Open API docs**, download the description from the publisher's site, and choose **Import a file instead** to import it from the **File** tab.

## Keep an API up to date

An API imported from a downloaded OpenAPI or Google Discovery description remembers where it came from, which groups you took and how its requests sign in. To bring in a new version, right-click the folder → **Update from API spec…**: the address it came from is filled in, new endpoints in your groups are added, and changed ones are updated where you haven't edited them. See [Update from the API spec](../import-export/#update-from-the-api-spec).

## For AI agents

Coding agents connected over [MCP](../../agents/setup/) use the catalog with `search_catalog` and `import_from_catalog`, with the same confirmations as in the app. See [Agent tools](../../agents/tools/).

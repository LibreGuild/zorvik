---
title: Dynamic variables
description: 170 built-in {{$...}} variables that generate IDs, timestamps, names, emails, addresses, prices, valid card numbers and more on every send, with arguments and exact formats.
sidebar:
  order: 2
---

Dynamic variables are built in. They start with `$` and produce a new value every time a request is sent, so you don't have to define them anywhere. Every one of Postman's dynamic variables is here with the same name and format, so imported collections work unchanged, plus many more for API testing: modern IDs, valid card and IBAN numbers, dates relative to now, and values you can shape with arguments.

```http
POST {{baseUrl}}/orders
X-Request-ID: {{$uuidv7}}
Content-Type: application/json

{
  "reference": "test-{{$timestamp}}",
  "customer": { "name": "{{$randomFullName}}", "email": "{{$randomEmail}}" },
  "quantity": {{$randomInt(1, 5)}},
  "price": {{$randomPrice(5, 50)}},
  "deliverBy": "{{$isoDate(+3d)}}",
  "size": "{{$randomFrom(S, M, L)}}"
}
```

Type `{{$` in any field to get them as suggestions, with an example value next to each. Hover one to see what it makes.

## Arguments

Many variables take arguments in parentheses. Without them they use sensible defaults.

| Form | Means |
|---|---|
| `{{$randomInt(1, 100)}}` | A number from 1 to 100 (both included). Without arguments: 0 to 1000. |
| `{{$randomFloat(0, 1, 3)}}` | Min, max and decimals |
| `{{$randomString(24)}}`, `{{$randomHex(32)}}` | A length |
| `{{$randomFrom(red, green, blue)}}` | One of the values. Quote a value to keep commas or spaces in it: `{{$randomFrom("a, b", c)}}` |
| `{{$timestamp(+1h)}}`, `{{$isoTimestamp(-7d)}}`, `{{$isoDate(+1w2d)}}` | Now, moved by an offset: `s`, `m`, `h`, `d`, `w`, combined as in `1w2d3h`; `0` or `now` is now |
| `{{$randomDatePast(30)}}` | Within the last 30 days (`$randomDateFuture`, `$randomDateRecent` likewise) |
| `{{$randomCreditCardNumber(visa)}}`, `{{$randomBankAccountIban(DE)}}` | A brand or country |

- Spaces around arguments are ignored, and `()` is the same as no arguments. An empty argument uses its default: `{{$randomInt(, 5)}}`.
- Arguments are plain text: `{{variables}}` inside them are not filled in.
- A wrong argument (a minimum above the maximum, an unknown country, a count too large) makes the variable **undefined**: it is sent as written and listed in the *Sent with undefined values* warning, like an unknown name.

## How they behave

- **A new value for every use.** Two `{{$uuid}}` in the same request get two different UUIDs. Every send, and every iteration of a collection run, gets new values.
- **They work wherever variables work**: the URL, headers, bodies, auth fields and [mock server](../../servers/templates/) templates. See [Where variables work](../variables-and-environments/#where-variables-work).
- **Test data only.** Emails use `example.com`, `example.net` and `example.org`, card numbers use the card brands' test ranges (they pass the Luhn check), and IBANs, ISBNs and EANs have valid check digits but belong to no one.
- **They are the lowest scope.** A variable you define with the same name, for example `$timestamp` in an environment, replaces the dynamic value. That's handy to pin a value while debugging.
- **Names are matched exactly first, then ignoring case**, so `{{$randomIpv6}}` finds `{{$randomIPV6}}`.
- **Scripts** get the same values: `pm.variables.replaceIn("{{$randomEmail}}")`.
- **The command line** generates them the same way (`zorvik run`, `zorvik load`). In load tests, requests that use them are rendered anew for every request. See [Load testing](../../load-testing/overview/).

## Using one value in several places

Because every use gets a new value, generate the value once in a **pre-request script** and use an ordinary variable instead:

```js title="Pre-request script"
pm.variables.set("orderId", pm.variables.replaceIn("{{$uuid}}"));
```

Then use `{{orderId}}` in the URL, headers and body: they all get the same UUID for this send. `pm.variables` values last for this send only (or the whole collection run). To keep a value for later requests, use `pm.environment.set` instead. See [Values set by scripts](../variables-and-environments/#values-set-by-scripts) and the [pm API reference](../../scripting/pm-reference/).

## Copy as cURL or code

When you [copy a request as cURL or code](../../requests/import-export/#copy-as-curl-or-code), dynamic variables are replaced with a freshly generated value, also when **Substitute variables** is off.

## All dynamic variables

<!-- catalog:start (written by `cargo test -p zorvik-workspace`: don't edit by hand) -->

### IDs

| Variable | Value | Example |
|---|---|---|
| `{{$guid}}` | A random UUID v4. | `611c2e81-2ccb-42d8-9ddc-2d0bfa65c1b4` |
| `{{$uuid}}` | A random UUID v4. | `3f1c9b0e-7d2a-4c8e-9a51-2b6f0d4e8c17` |
| `{{$randomUUID}}` | A random UUID v4. | `6929bb52-3ab2-448a-9796-d6480ecad36b` |
| `{{$uuidv7}}` | A time-ordered UUID v7: IDs made later sort after earlier ones. | `01992f3a-8b4c-7d2e-9f10-3a5b7c9d1e2f` |
| `{{$ulid}}` | A ULID: 26 characters that sort by creation time. | `01K67Q3K7V5X2M9N4P6R8T0W1Y` |
| `{{$nanoid}}`<br/>`{{$nanoid(size)}}` | A URL-safe Nano ID, 21 characters unless you give a size. | `V1StGXR8_Z5jdHi6B-myT` |
| `{{$objectId}}` | A MongoDB ObjectId: 24 hex characters that start with the creation time. | `66f7c2a4e1b3d59f0a2c4e81` |
| `{{$snowflake}}` | A Twitter-style snowflake ID: a 64-bit number that grows with time. | `1972248110573953024` |
| `{{$traceparent}}` | A W3C Trace Context traceparent header value with random trace and span IDs. | `00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01` |
| `{{$randomHex}}`<br/>`{{$randomHex(length)}}` | Random lowercase hex, 32 characters unless you give a length. | `9f86d081884c7d659a2feaa0c55ad015` |

### Numbers

| Variable | Value | Example |
|---|---|---|
| `{{$randomInt}}`<br/>`{{$randomInt(min, max)}}` | A random integer from 0 to 1000, or from min to max. | `802` |
| `{{$randomFloat}}`<br/>`{{$randomFloat(min, max, decimals)}}` | A random number from 0 to 1000 with 2 decimals, or in your range and precision. | `531.27` |
| `{{$randomDigits}}`<br/>`{{$randomDigits(length)}}` | 6 random digits (leading zeros kept) for codes and PINs, or as many as you ask for. | `042917` |
| `{{$randomInt64}}` | A random signed 64-bit integer, usually beyond JavaScript's safe integer range. | `-7239104857163029471` |
| `{{$randomBoolean}}` | true or false. | `true` |

### Text

| Variable | Value | Example |
|---|---|---|
| `{{$randomAlphaNumeric}}`<br/>`{{$randomAlphaNumeric(length)}}` | One random lowercase letter or digit, or as many as you ask for. | `y` |
| `{{$randomString}}`<br/>`{{$randomString(length)}}` | Random letters and digits, 16 characters unless you give a length. | `aZ3kQ9xLm2PvT8wR` |
| `{{$randomPassword}}`<br/>`{{$randomPassword(length)}}` | A random 15-character alphanumeric password, or the length you give. | `t9iXe7COoDKv8k3` |
| `{{$randomStrongPassword}}`<br/>`{{$randomStrongPassword(length)}}` | A 16-character password (or 4 and more you choose) with upper and lower case letters, digits and symbols. | `q7#Rt2!mZx9@Lp4&` |
| `{{$randomBase64}}`<br/>`{{$randomBase64(bytes)}}` | Base64 of 16 random bytes, or of as many bytes as you ask for. | `q83vEjRWeJq8z0Ed0ZrM3A==` |
| `{{$randomUnicodeString}}`<br/>`{{$randomUnicodeString(length)}}` | 16 characters (or your length) mixing accents, Greek, Cyrillic, CJK, right-to-left scripts and emoji. | `éñ東京שלום😀Жω한مर🎉ß` |
| `{{$randomEmoji}}` | A random emoji; some are several code points (skin tones, flags, joined sequences). | `🚀` |
| `{{$randomFrom}}`<br/>`{{$randomFrom(values…)}}` | One of the values you list, e.g. $randomFrom(red, green, blue). | `green` |
| `{{$randomSlug}}`<br/>`{{$randomSlug(words)}}` | A URL slug of 3 random words, or of as many words as you ask for. | `distributed-token-cache` |
| `{{$randomAbbreviation}}` | A random tech abbreviation. | `JSON` |

### Words

| Variable | Value | Example |
|---|---|---|
| `{{$randomNoun}}` | A random noun. | `bandwidth` |
| `{{$randomVerb}}` | A random verb. | `parse` |
| `{{$randomIngverb}}` | A random verb ending in -ing. | `navigating` |
| `{{$randomAdjective}}` | A random adjective. | `multi-byte` |
| `{{$randomWord}}` | A random word. | `matrix` |
| `{{$randomWords}}`<br/>`{{$randomWords(count)}}` | 2 to 5 random words, or as many as you ask for. | `synergies token bandwidth` |
| `{{$randomPhrase}}` | A random tech phrase. | `You can't parse the monitor without navigating the neural SQL feed!` |

### Lorem ipsum

| Variable | Value | Example |
|---|---|---|
| `{{$randomLoremWord}}` | A word of lorem ipsum. | `est` |
| `{{$randomLoremWords}}`<br/>`{{$randomLoremWords(count)}}` | 3 words of lorem ipsum, or as many as you ask for. | `vel repellat nobis` |
| `{{$randomLoremSentence}}`<br/>`{{$randomLoremSentence(words)}}` | A sentence of lorem ipsum, 3 to 10 words unless you give a number. | `Molestias consequuntur nisi non quod.` |
| `{{$randomLoremSentences}}`<br/>`{{$randomLoremSentences(count)}}` | 2 to 6 sentences of lorem ipsum, or as many as you ask for. | `Et sint voluptas similique iure. Amet perspiciatis quia rem.` |
| `{{$randomLoremParagraph}}`<br/>`{{$randomLoremParagraph(sentences)}}` | A paragraph of 3 to 6 lorem ipsum sentences, or as many sentences as you ask for. | `Ab aliquid odio iste quo voluptas. Voluptatem quia natus est. Minus rerum ut eos dolorem.` |
| `{{$randomLoremParagraphs}}`<br/>`{{$randomLoremParagraphs(count)}}` | 3 paragraphs of lorem ipsum on separate lines, or as many as you ask for. | `Voluptatem rem magnam aliquam ab id. Aut quaerat qui. Nemo quia et. Odio ut ea est. Quo sint vel. Nisi velit id.` |
| `{{$randomLoremText}}` | A random amount of lorem ipsum text. | `Quisquam asperiores exercitationem ut ipsum. Aut eius molestias.` |
| `{{$randomLoremSlug}}`<br/>`{{$randomLoremSlug(words)}}` | A URL slug of 3 lorem ipsum words, or of as many words as you ask for. | `eos-aperiam-accusamus` |
| `{{$randomLoremLines}}`<br/>`{{$randomLoremLines(count)}}` | 1 to 5 lines of lorem ipsum, or as many as you ask for. | `Ducimus in ut mollitia. A itaque non.` |

### Dates & times

| Variable | Value | Example |
|---|---|---|
| `{{$timestamp}}`<br/>`{{$timestamp(offset)}}` | The current Unix time in seconds; an offset such as +1h or -7d moves it. | `1790588467` |
| `{{$timestampMs}}`<br/>`{{$timestampMs(offset)}}` | The current Unix time in milliseconds; an offset such as +1h or -7d moves it. | `1790588467123` |
| `{{$isoTimestamp}}`<br/>`{{$isoTimestamp(offset)}}` | The current time in ISO 8601 (UTC, milliseconds); an offset such as -7d moves it. | `2026-09-28T09:41:07.123Z` |
| `{{$isoDate}}`<br/>`{{$isoDate(offset)}}` | Today's date in UTC as YYYY-MM-DD; an offset such as +30d moves it. | `2026-09-28` |
| `{{$today}}` | Today's date in UTC as YYYY-MM-DD. | `2026-09-28` |
| `{{$tomorrow}}` | Tomorrow's date in UTC as YYYY-MM-DD. | `2026-09-29` |
| `{{$yesterday}}` | Yesterday's date in UTC as YYYY-MM-DD. | `2026-09-27` |
| `{{$httpDate}}`<br/>`{{$httpDate(offset)}}` | The current time as an HTTP date, for headers such as If-Modified-Since; takes an offset. | `Mon, 28 Sep 2026 09:41:07 GMT` |
| `{{$randomDateTime}}`<br/>`{{$randomDateTime(from, to)}}` | A random ISO 8601 time within a year of now, or between two offsets such as (-30d, now). | `2026-03-14T16:22:05.481Z` |
| `{{$randomDate}}`<br/>`{{$randomDate(from, to)}}` | A random date (YYYY-MM-DD) within a year of now, or between two offsets such as (now, +90d). | `2026-11-03` |
| `{{$randomTime}}` | A random time of day, HH:MM:SS. | `14:23:05` |
| `{{$randomDatePast}}`<br/>`{{$randomDatePast(days)}}` | A random moment in the past year (or past days you give), in JavaScript date format. | `Mon Mar 02 2026 09:09:26 GMT+0000` |
| `{{$randomDateFuture}}`<br/>`{{$randomDateFuture(days)}}` | A random moment in the next year (or next days you give), in JavaScript date format. | `Wed Mar 17 2027 13:11:50 GMT+0000` |
| `{{$randomDateRecent}}`<br/>`{{$randomDateRecent(days)}}` | A random moment in the last day (or last days you give), in JavaScript date format. | `Sun Sep 27 2026 23:12:37 GMT+0000` |
| `{{$randomWeekday}}` | A random day of the week. | `Thursday` |
| `{{$randomMonth}}` | A random month. | `February` |
| `{{$randomTimezone}}` | A random IANA time zone. | `Asia/Kolkata` |

### People

| Variable | Value | Example |
|---|---|---|
| `{{$randomFirstName}}` | A random first name. | `Ethan` |
| `{{$randomLastName}}` | A random last name. | `Schneider` |
| `{{$randomFullName}}` | A random first and last name. | `Priya Fernández` |
| `{{$randomNamePrefix}}` | A random name prefix. | `Dr.` |
| `{{$randomNameSuffix}}` | A random name suffix. | `MD` |
| `{{$randomGender}}` | female, male or non-binary. | `female` |
| `{{$randomBirthdate}}`<br/>`{{$randomBirthdate(minAge, maxAge)}}` | A date of birth (YYYY-MM-DD) of someone aged 18 to 80, or in your age range. | `1987-04-12` |
| `{{$randomAge}}`<br/>`{{$randomAge(min, max)}}` | A random age from 18 to 80, or in your range. | `34` |
| `{{$randomPhoneNumber}}` | A ten-digit phone number. | `704-261-3424` |
| `{{$randomPhoneNumberExt}}` | A phone number with a two-digit extension. | `27-299-983-3864` |
| `{{$randomE164Phone}}`<br/>`{{$randomE164Phone(country)}}` | A mobile number in E.164 format from a random country, or from a country code such as DE. | `+14155552671` |
| `{{$randomJobArea}}` | A random job area. | `Mobility` |
| `{{$randomJobDescriptor}}` | A random job descriptor. | `Senior` |
| `{{$randomJobTitle}}` | A random job title. | `International Creative Liaison` |
| `{{$randomJobType}}` | A random job type. | `Coordinator` |

### Internet

| Variable | Value | Example |
|---|---|---|
| `{{$randomEmail}}` | A random email address at a reserved example domain, so no real inbox gets mail. | `pablo.garcia62@example.com` |
| `{{$randomExampleEmail}}` | A random email address at example.com, example.net or example.org. | `Talon.Weber28@example.net` |
| `{{$randomUserName}}` | A random username. | `Lottie.Smith24` |
| `{{$randomDomainName}}` | A random domain name. | `hoffmann.io` |
| `{{$randomDomainSuffix}}` | A random top-level domain. | `org` |
| `{{$randomDomainWord}}` | A random domain name without the suffix. | `meyer` |
| `{{$randomUrl}}` | A random URL. | `https://schmidt.net` |
| `{{$randomIP}}` | A random IPv4 address. | `241.102.234.100` |
| `{{$randomIPV6}}` | A random IPv6 address. | `dbe2:7ae6:119b:c161:1560:6dda:3a9b:90a9` |
| `{{$randomPrivateIP}}` | A private IPv4 address (10.0.0.0/8, 172.16.0.0/12 or 192.168.0.0/16). | `192.168.14.203` |
| `{{$randomMACAddress}}` | A random MAC address. | `32:d4:68:5f:b4:c7` |
| `{{$randomPort}}` | A random non-privileged port, 1024 to 65535. | `51837` |
| `{{$randomProtocol}}` | http or https. | `https` |
| `{{$randomHttpMethod}}` | A random HTTP method. | `PATCH` |
| `{{$randomHttpStatus}}` | A common HTTP status code. | `404` |
| `{{$randomUserAgent}}` | A current browser user agent (Chrome, Edge, Firefox, Safari; desktop and mobile). | `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36` |
| `{{$randomSemver}}` | A random semantic version number. | `7.0.5` |

### Location

| Variable | Value | Example |
|---|---|---|
| `{{$randomCountry}}` | A random country. | `Kazakhstan` |
| `{{$randomCountryCode}}` | A two-letter country code (ISO 3166-1 alpha-2). | `CV` |
| `{{$randomCountryCodeAlpha3}}` | A three-letter country code (ISO 3166-1 alpha-3). | `CPV` |
| `{{$randomCity}}` | A random city. | `Lisbon` |
| `{{$randomStreetName}}` | A random street name. | `Maple Avenue` |
| `{{$randomStreetAddress}}` | A random street address. | `5742 Maple Avenue` |
| `{{$randomState}}` | A random US state. | `California` |
| `{{$randomStateAbbr}}` | A random US state code. | `CA` |
| `{{$randomPostalCode}}` | A five-digit postal code, the format used in the US, Germany, France, Spain, Italy and more. | `94107` |
| `{{$randomLatitude}}`<br/>`{{$randomLatitude(min, max)}}` | A random latitude, or one between min and max. | `55.2099` |
| `{{$randomLongitude}}`<br/>`{{$randomLongitude(min, max)}}` | A random longitude, or one between min and max. | `-159.9757` |
| `{{$randomCoordinates}}` | A random latitude,longitude pair. | `52.5200,13.4050` |
| `{{$randomLocale}}` | A two-letter language code (ISO 639-1). | `sr` |
| `{{$randomLanguageCode}}` | A two-letter language code (ISO 639-1). | `de` |
| `{{$randomLanguageTag}}` | A BCP 47 language tag, as sent in Accept-Language. | `pt-BR` |

### Finance

| Variable | Value | Example |
|---|---|---|
| `{{$randomBankAccount}}` | A random 8-digit bank account number. | `09454073` |
| `{{$randomBankAccountName}}` | A random bank account name. | `Home Loan Account` |
| `{{$randomBankAccountIban}}`<br/>`{{$randomBankAccountIban(country)}}` | An IBAN with valid check digits from AT, BE, CH, DE, DK, ES, FR, GB, IE, NL or SE, or the country you give. | `DE89370400440532013000` |
| `{{$randomBankAccountBic}}` | A random BIC (SWIFT code). | `EZIAUGJ1` |
| `{{$randomCreditCardMask}}` | The last four digits of a card number. | `3622` |
| `{{$randomCreditCardNumber}}`<br/>`{{$randomCreditCardNumber(brand)}}` | A card number that passes the Luhn check, on well-known test prefixes; brand: visa, mastercard, amex, discover, jcb or diners. | `4111111111111111` |
| `{{$randomCreditCardCvv}}` | A random three-digit card security code. | `839` |
| `{{$randomCreditCardExpiry}}` | A card expiry date in the next five years, MM/YY. | `08/29` |
| `{{$randomTransactionType}}` | A random transaction type. | `payment` |
| `{{$randomCurrencyCode}}` | A three-letter currency code (ISO 4217). | `EUR` |
| `{{$randomCurrencyName}}` | A random currency name. | `Pound Sterling` |
| `{{$randomCurrencySymbol}}` | A random currency symbol. | `£` |
| `{{$randomBitcoin}}` | A Bitcoin address with a valid checksum. | `1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa` |

### Business

| Variable | Value | Example |
|---|---|---|
| `{{$randomCompanyName}}` | A random company name. | `Weber GmbH` |
| `{{$randomCompanySuffix}}` | A random company suffix. | `LLC` |
| `{{$randomBs}}` | A random phrase of business-speak. | `harness frictionless platforms` |
| `{{$randomBsAdjective}}` | A random business-speak adjective. | `frictionless` |
| `{{$randomBsBuzz}}` | A random business-speak buzzword. | `repurpose` |
| `{{$randomBsNoun}}` | A random business-speak noun. | `e-services` |
| `{{$randomCatchPhrase}}` | A random catchphrase. | `Future-proofed heuristic open architecture` |
| `{{$randomCatchPhraseAdjective}}` | A random catchphrase adjective. | `Self-enabling` |
| `{{$randomCatchPhraseDescriptor}}` | A random catchphrase descriptor. | `bandwidth-monitored` |
| `{{$randomCatchPhraseNoun}}` | A random catchphrase noun. | `secured line` |

### Commerce

| Variable | Value | Example |
|---|---|---|
| `{{$randomPrice}}`<br/>`{{$randomPrice(min, max)}}` | A price from 0.00 to 1000.00, or between min and max. | `531.55` |
| `{{$randomProduct}}` | A random product. | `Towels` |
| `{{$randomProductAdjective}}` | A random product adjective. | `Incredible` |
| `{{$randomProductMaterial}}` | A random product material. | `Bamboo` |
| `{{$randomProductName}}` | A random product name. | `Handmade Concrete Chair` |
| `{{$randomDepartment}}` | A random store department. | `Electronics` |
| `{{$randomIsbn10}}` | An ISBN-10 with a valid check digit. | `0306406152` |
| `{{$randomIsbn13}}` | An ISBN-13 with a valid check digit. | `9780306406157` |
| `{{$randomEan13}}` | An EAN-13 barcode number with a valid check digit. | `4006381333931` |
| `{{$randomUpc}}` | A UPC-A barcode number with a valid check digit. | `036000291452` |

### Files

| Variable | Value | Example |
|---|---|---|
| `{{$randomFileName}}` | A random file name, extensions of all kinds. | `heuristic_socket.gltf` |
| `{{$randomFileType}}` | A random file type (the first part of a MIME type). | `model` |
| `{{$randomFileExt}}` | A random file extension. | `woff2` |
| `{{$randomCommonFileName}}` | A random file name with a common extension. | `well_modulated_driver.mp4` |
| `{{$randomCommonFileType}}` | A common file type (the first part of a MIME type). | `image` |
| `{{$randomCommonFileExt}}` | A common file extension. | `png` |
| `{{$randomFilePath}}` | A random absolute file path. | `/var/log/modular_payload.json` |
| `{{$randomDirectoryPath}}` | A random directory path. | `/usr/local/bin` |
| `{{$randomMimeType}}` | A random MIME type. | `application/json` |

### Databases

| Variable | Value | Example |
|---|---|---|
| `{{$randomDatabaseColumn}}` | A random database column name. | `updatedAt` |
| `{{$randomDatabaseType}}` | A random database column type. | `varchar` |
| `{{$randomDatabaseCollation}}` | A random database collation. | `utf8mb4_unicode_ci` |
| `{{$randomDatabaseEngine}}` | A random database engine. | `InnoDB` |

### Images

| Variable | Value | Example |
|---|---|---|
| `{{$randomAvatarImage}}` | The URL of a random avatar image. | `https://avatars.githubusercontent.com/u/30218384` |
| `{{$randomImageUrl}}`<br/>`{{$randomImageUrl(width, height)}}` | The URL of a random 640×480 image, or of the size you give. | `https://picsum.photos/seed/k3v9x2qa/640/480` |
| `{{$randomAbstractImage}}`<br/>`{{$randomAbstractImage(width, height)}}` | The URL of a random abstract image. | `https://loremflickr.com/640/480/abstract?lock=4821` |
| `{{$randomAnimalsImage}}`<br/>`{{$randomAnimalsImage(width, height)}}` | The URL of a random animal image. | `https://loremflickr.com/640/480/animals?lock=77` |
| `{{$randomBusinessImage}}`<br/>`{{$randomBusinessImage(width, height)}}` | The URL of a random business image. | `https://loremflickr.com/640/480/business?lock=3190` |
| `{{$randomCatsImage}}`<br/>`{{$randomCatsImage(width, height)}}` | The URL of a random cat image. | `https://loremflickr.com/640/480/cats?lock=15023` |
| `{{$randomCityImage}}`<br/>`{{$randomCityImage(width, height)}}` | The URL of a random city image. | `https://loremflickr.com/640/480/city?lock=908` |
| `{{$randomFoodImage}}`<br/>`{{$randomFoodImage(width, height)}}` | The URL of a random food image. | `https://loremflickr.com/640/480/food?lock=6412` |
| `{{$randomNightlifeImage}}`<br/>`{{$randomNightlifeImage(width, height)}}` | The URL of a random nightlife image. | `https://loremflickr.com/640/480/nightlife?lock=221` |
| `{{$randomFashionImage}}`<br/>`{{$randomFashionImage(width, height)}}` | The URL of a random fashion image. | `https://loremflickr.com/640/480/fashion?lock=5566` |
| `{{$randomPeopleImage}}`<br/>`{{$randomPeopleImage(width, height)}}` | The URL of a random image of people. | `https://loremflickr.com/640/480/people?lock=42` |
| `{{$randomNatureImage}}`<br/>`{{$randomNatureImage(width, height)}}` | The URL of a random nature image. | `https://loremflickr.com/640/480/nature?lock=8080` |
| `{{$randomSportsImage}}`<br/>`{{$randomSportsImage(width, height)}}` | The URL of a random sports image. | `https://loremflickr.com/640/480/sports?lock=1234` |
| `{{$randomTransportImage}}`<br/>`{{$randomTransportImage(width, height)}}` | The URL of a random transport image. | `https://loremflickr.com/640/480/transport?lock=97` |
| `{{$randomImageDataUri}}`<br/>`{{$randomImageDataUri(width, height)}}` | A random single-color SVG image as a data URI. | `data:image/svg+xml;charset=UTF-8,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%22640%22%20height%3D%22480%22%3E%3C%2Fsvg%3E` |

### Colors

| Variable | Value | Example |
|---|---|---|
| `{{$randomColor}}` | A random color name. | `fuchsia` |
| `{{$randomHexColor}}` | A random hex color. | `#47594a` |
| `{{$randomRgbColor}}` | A random CSS rgb() color. | `rgb(71, 89, 74)` |
| `{{$randomHslColor}}` | A random CSS hsl() color. | `hsl(210, 45%, 38%)` |

<!-- catalog:end -->

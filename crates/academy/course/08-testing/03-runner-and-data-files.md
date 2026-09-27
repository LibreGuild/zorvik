---
id: runner-and-data-files
title: The collection runner and data files
summary: Run a whole folder in order with one click, and feed it a CSV file to test many inputs at once.
minutes: 6
lab:
  title: Three signups, one run
  goal: Build a signup request that reads its values from a CSV file, and run it once per row.
  minutes: 10
  servers:
    api:
      name: Signup API
      kind: http
      http:
        routes:
          - method: POST
            path: /signups
            matchBody: "@"
            status: 201
            headers:
              - key: Content-Type
                value: application/json
            body: '{"id": "{{$uuid}}", "welcome": true}'
          - method: POST
            path: /signups
            status: 400
            headers:
              - key: Content-Type
                value: application/json
            body: '{"error": "That email address does not look right"}'
  files:
    data/signups.csv: |
      email,plan,expectedStatus
      ada@example.com,pro,201
      grace@example.com,free,201
      not-an-email,free,400
  steps:
    - text: |
        Create a folder **Signups** (collection **+** → **New folder**). Right-click it → **New HTTP request**, name it **Sign up**, and make it `POST {{api}}/signups` with this **JSON** body:

        ```json
        {"email": "{{email}}", "plan": "{{plan}}"}
        ```

        Save it (**⌘/Ctrl + S**). `{{email}}` and `{{plan}}` don't exist yet: the data file will fill them in.
      hints:
        - The body goes in the Body tab. Pick JSON as its type first.
        - Method POST, URL {{api}}/signups, Body → JSON, paste the line above, then ⌘S.
        - "Folder Signups, request Sign up inside it, POST, JSON body with {{email}} and {{plan}}. Save; the tab's dot disappears."
      check:
        saved:
          request: { name: Sign up, path: "Signups/*", method: POST, url: "*/signups", body: { text: "*{{email}}*" } }
      solution:
        - call: { method: folder.create, params: { parent: "", name: Signups } }
        - call:
            method: request.create
            params:
              parent: Signups
              request:
                name: Sign up
                method: POST
                url: "{{api}}/signups"
                body: { type: json, text: '{"email": "{{email}}", "plan": "{{plan}}"}' }
    - text: |
        The last row of the data file is a bad email address, and the right answer to it is **400**. Add a **Post-response** test that expects whatever status the row says, then save:

        ```js
        pm.test("Status is as expected", () => {
          const expected = Number(pm.iterationData.get("expectedStatus"));
          pm.response.to.have.status(expected);
        });
        ```
      hints:
        - pm.iterationData holds the current row of the data file. CSV values are text, so Number() turns "201" into 201.
        - Open Sign up → Scripts → Post-response, paste the test and press ⌘S.
        - "The test is saved when the tab has no dot. Without it, the 400 for the bad address would count as a failure."
      check:
        saved:
          request: { name: Sign up, path: "Signups/*", scripts: { postResponse: "*expectedStatus*" } }
      solution:
        - call:
            method: request.save
            params:
              path: Signups/Sign up.yaml
              request:
                name: Sign up
                seq: 1
                method: POST
                url: "{{api}}/signups"
                body: { type: json, text: '{"email": "{{email}}", "plan": "{{plan}}"}' }
                scripts:
                  postResponse: |
                    pm.test("Status is as expected", () => {
                      const expected = Number(pm.iterationData.get("expectedStatus"));
                      pm.response.to.have.status(expected);
                    });
    - text: Right-click **Signups** → **Run…**. Under **Data file**, press **Choose file…** and pick `data/signups.csv` in the Training Bootcamp workspace folder. The preview shows 3 rows. Press **Run** (**⌘/Ctrl + Enter**).
      hints:
        - Iterations can stay empty; the runner then does one iteration per row.
        - "The file is in the workspace folder, in data/. To find the folder, open the workspace menu (Training Bootcamp, top left) → Workspace settings…: Location shows its path. In the file dialog on macOS, press ⌘⇧G and paste it; on Windows, paste it into the address bar."
        - "Data file: data/signups.csv (3 rows), then Run. Three iterations, three tests passed, including the 400 for not-an-email."
      check:
        all:
          - run: { name: Signups, iterations: 3, requests: 3, passed: true, testsPassed: 3 }
          - request: { server: api, method: POST, path: /signups, status: 400, json: { email: not-an-email } }
      solution:
        - call: { method: runner.start, params: { folder: Signups, dataFile: data/signups.csv } }
        - wait: 500
quiz:
  - question: A data file has 5 rows and you leave **Iterations** empty. How many times does each request in the folder run?
    options:
      - Once
      - Until you press Stop
      - 5 times, once per row
    answer: 2
    explain: One iteration per row. In iteration 3, `{{email}}` holds row 3's email.
  - question: In the runner, a request has no tests and the server answers 404. Does it pass?
    options:
      - No, without tests a status of 400 or more fails the request
      - Yes, the server answered
      - Only if it is the last request
    answer: 0
    explain: Without tests, the status decides. With tests, the tests decide, which is how a request that expects a 400 can pass.
  - question: Why put a bad email address in the data file on purpose?
    options:
      - To make the run slower
      - To check the API refuses bad input, which is as important as accepting good input
      - Data files must contain one error
    answer: 1
    explain: Testing what should fail is called negative testing. An API that accepts "not-an-email" is a bug waiting to happen.
---

Sending requests one by one is fine while you explore. But once you have ten requests that belong together, like *log in, create an order, pay, check the invoice*, you want to run them all, in order, with their tests, in one go. That's the **collection runner**.

## Running a folder

Right-click a folder → **Run…** (or the collection's **+** menu → **Run…** for everything). A runner tab named after the folder opens, with:

- the folder's requests in order, with a checkbox for each; drag them to change the order,
- **Iterations**: how many times to run the whole list, and **Delay (ms)** between requests,
- **Data file**: a CSV or JSON file to drive the run (below),
- **Stop on the first failure**.

Press **Run** (**⌘/Ctrl + Enter**) and results arrive live: every request with its status, time and tests. Scripts run exactly as when you press Send, so chains of `pm.environment.set` work from one request to the next.

A request **passes** when it was sent, its scripts didn't crash, and its tests passed. A request without tests passes when the status is below 400.

## Data files

A **data file** runs the same requests with different inputs. It's a CSV file (a table saved as text, like a spreadsheet export) or a JSON list of objects. Each **row** is one **iteration**, and each **column** becomes a variable:

```anatomy
email,plan,expectedStatus | header row: the column names become variable names
ada@example.com,pro,201 | iteration 1: {{email}} is ada@example.com
grace@example.com,free,201 | iteration 2
not-an-email,free,400 | iteration 3: bad input, and 400 is the right answer
```

Use a column as `{{email}}` anywhere in the request, or read it in a script with `pm.iterationData.get("email")`.

```flow
Row 1 -> Sign up -> 201, test passed
Row 2 -> Sign up -> 201, test passed
Row 3 -> Sign up -> 400, test passed
```

> [!note] Think of it like…
> A mail merge. You write one letter with blanks for the name and address, and give it a list of people. Out come a hundred personal letters. The request is the letter, the data file is the list.

## Test the unhappy path too

Good data files include the inputs that *should* fail: an email without an @, a price of -1, a name 5,000 characters long. Put the expected status in its own column, like `expectedStatus`, and one test checks every row. This is called **data-driven testing**.

> [!tip] Keep data files in the workspace
> Save data files inside your workspace folder (a `data/` folder works well), so they're in Git with the requests that use them. Zorvik only reads data files from outside the workspace if you allow it in Settings.

When a run finishes, **Export** saves it as a JSON report or as **JUnit XML**, a format CI servers understand. You'll use that in the automation unit.

**You'll use this when…** a form accepts fifty kinds of input. Instead of fifty manual sends, you keep one request and a fifty-row CSV, and every run tells you, row by row, which input the API gets wrong.

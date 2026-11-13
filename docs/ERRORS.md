# Error codes

Every error from the RGS API looks like this:

```json
{ "error": "update_required", "message": "Update Lanternfall to keep playing. You have version 0.2, the newest is 0.3." }
```

Games should show `message` to the player as is (it's written for players), and can use
`error` to decide what to do, like showing an "Update" button. New codes may be added; treat
unknown ones like `internal`.

Shown in-game as `Error: <message>`, for example:

- `Error: Update Lanternfall to keep playing. You have version 0.2, the newest is 0.3.`
- `Error: RGS is rebooting right now. Try again in a few minutes.`

| Code | HTTP | When | Example message |
|---|---|---|---|
| `update_required` | 426 | The game is older than its `min_version` | Update Lanternfall to keep playing. You have version 0.2, the newest is 0.3. |
| `maintenance` | 503 | A restart or maintenance is under way | RGS is rebooting right now. Try again in a few minutes. |
| `banned` | 403 | The account is banned | This account is banned for another 2 hours. Reason: ... |
| `unauthorized` | 401 | Not signed in, login expired, wrong password or 2FA code | Your login has expired. Sign in again. |
| `forbidden` | 403 | Signed in, but not allowed | Only staff can use the admin panel. |
| `rate_limited` | 429 | Too many tries in a short time | Too many tries. Wait a minute. |
| `bad_request` | 400 | Something in the request is wrong | Usernames are 3 to 20 letters, numbers or underscores. |
| `conflict` | 409 | Already exists or already done | That username is taken. |
| `not_found` | 404 | No such thing | Not found. |
| `internal` | 500 | A bug or outage on the server | Something went wrong on the server. |

When the server can't be reached at all, there's no response to read, so games show their own
message, e.g. `Error: Can't reach RGS. Check your internet connection.`

## Announcements

Games poll `GET /api/v1/status` every minute or so (it works even during maintenance):

```json
{
  "status": "maintenance_scheduled",
  "maintenance": {
    "starts_at": 1793500000, "seconds_left": 2700, "restart": true, "reason": "Update 0.3",
    "announcement": { "step": 45, "text": "Servers restarting in 45 minutes (Update 0.3).", "show": true }
  },
  "games": [ { "id": "lanternfall", "name": "Lanternfall", "latest_version": "0.3", "min_version": "0.3" } ]
}
```

Show `announcement.text` whenever `announcement.step` changes (60, 45, 30, 15, 10, 5, 1, then 0
when it starts). `status` is `online`, `maintenance_scheduled` or `maintenance`. Games can also
compare their own version with `latest_version` to say "a new version is out" before it's
required.

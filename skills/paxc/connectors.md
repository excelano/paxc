# Connector bodies for the common Standard connectors

What an agent needs to author a connector call is the operation's `operationId`
and its parameter keys. Both are public API surface, published in Microsoft's
connector reference; neither is tenant data. This page carries verified bodies
for the operations that come up most, so the shape does not have to be guessed
and does not have to be lifted out of somebody's export.

Everything here is Standard-licensed. The typical target tenant has no Premium
connectors, so nothing on this page needs one.

Each body is the whole contents of one file under `pa/` next to the source. Drop
it in, replace the angle-bracket placeholders, and reference it from pax with
`pa <Name>` — except the triggers, which are picked up from the filename alone
and are never named in source.

## The envelope

Every connector body, action or trigger, has the same four parts:

```json
{
  "type": "OpenApiConnection",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_sharepointonline",
      "connectionName": "shared_sharepointonline",
      "operationId": "GetItems"
    },
    "parameters": {}
  }
}
```

`type` is `OpenApiConnection` for a plain call, `OpenApiConnectionWebhook` for
one that registers a callback and waits (Approvals, Forms), and
`OpenApiConnectionNotification` for a push trigger (new mail). Each body below
says which it is.

Two fields belong to the packager, not to you. Do not write
`inputs.authentication` — PA's exporter emits it and PA's importer rejects it,
so paxc strips it. Do not write `host.connectionReferenceName` — the importer
requires it, and paxc derives it from `connectionName`. Both are handled for
every `OpenApiConnection*` type, so a body copied from this page needs no
further adjustment before `paxc --target pa-legacy`.

`connectionName` must match a key in `pa/connectionReferences.json`. Using the
`shared_*` API name for both is what PA's own designer does, and it is what the
bodies below assume.

### Expressions inside a body

A parameter whose value is nothing but an expression takes the bare form,
`"@variables('total')"`. A parameter that mixes text and expressions takes the
interpolating form, one pair of braces per expression:
`"<p>Hello @{variables('name')}</p>"`. The distinction is not cosmetic — the
bare form preserves the value's type, which is what an integer `id` or a boolean
needs, while the braced form always produces a string. PA writes both, and the
bodies below follow the same rule.

This is PA's expression syntax, single quotes and all. It is correct inside
`pa/*.json` and a parse error in pax source, where strings are double-quoted.

## `pa/connectionReferences.json`

One entry per connector the flow touches. A flow with a SharePoint trigger and
an Outlook action needs both:

`pa/connectionReferences.json`

```json
{
  "shared_sharepointonline": {
    "id": "/providers/Microsoft.PowerApps/apis/shared_sharepointonline",
    "apiName": "sharepointonline"
  },
  "shared_office365": {
    "id": "/providers/Microsoft.PowerApps/apis/shared_office365",
    "apiName": "office365"
  }
}
```

An export carries three more fields per entry — the tenant's own connection id,
`source`, and `tier`. Leave them out when authoring. paxc generates
package-local resource GUIDs for the API and the connection, and the import
dialog prompts the user to bind each one to a connection in their tenant. That
binding is the one step of the loop a human has to perform, and it is where it
belongs: consent for an agent's flow to send mail as someone is a decision for
the someone.

## Placeholders

`<SITE>` is a full site URL, `https://contoso.sharepoint.com/sites/Operations`.
`<LIST>` is the list identifier PA's designer writes as a GUID; a user reads it
off **List settings → the `List=` value in the address bar**. `<FORM_ID>` comes
off the Forms URL. `<TEAM_ID>` and `<CHANNEL_ID>` come from the Teams channel
link. Anything else in angle brackets is a plain value the user supplies.

## SharePoint — `shared_sharepointonline`

### When an item is created

`pa/When_an_item_is_created.trigger.json`

```json
{
  "type": "OpenApiConnection",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_sharepointonline",
      "connectionName": "shared_sharepointonline",
      "operationId": "GetOnNewItems"
    },
    "parameters": {
      "dataset": "<SITE>",
      "table": "<LIST>"
    }
  },
  "recurrence": { "frequency": "Minute", "interval": 1 },
  "splitOn": "@triggerOutputs()?['body/value']"
}
```

This one polls, so it carries its own `recurrence`, and `splitOn` is what makes
the flow run once per new item rather than once per batch. Swap `GetOnNewItems`
for `GetOnUpdatedItems` to fire on creation *and* every later edit; the
parameters and the rest of the envelope are identical.

Inside the flow, the item is `triggerBody()` — a single item, because `splitOn`
already unwrapped the batch.

### Get items

`pa/Get_items.json`

```json
{
  "type": "OpenApiConnection",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_sharepointonline",
      "connectionName": "shared_sharepointonline",
      "operationId": "GetItems"
    },
    "parameters": {
      "dataset": "<SITE>",
      "table": "<LIST>",
      "$filter": "Status eq 'Open'",
      "$orderby": "Created desc",
      "$top": 500
    }
  }
}
```

`$filter`, `$orderby` and `$top` are all optional; drop the keys you do not
need rather than passing an empty string. The rows come back as
`body("Get_items")?["value"]`, which is what a `foreach` iterates.

### Get item

`pa/Get_item.json`

```json
{
  "type": "OpenApiConnection",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_sharepointonline",
      "connectionName": "shared_sharepointonline",
      "operationId": "GetItem"
    },
    "parameters": {
      "dataset": "<SITE>",
      "table": "<LIST>",
      "id": "@triggerBody()?['ID']"
    }
  }
}
```

### Create item

`pa/Create_item.json`

```json
{
  "type": "OpenApiConnection",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_sharepointonline",
      "connectionName": "shared_sharepointonline",
      "operationId": "PostItem"
    },
    "parameters": {
      "dataset": "<SITE>",
      "table": "<LIST>",
      "item/Title": "@variables('title')",
      "item/DueDate": "@variables('due')"
    }
  }
}
```

### Update item

`pa/Update_item.json`

```json
{
  "type": "OpenApiConnection",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_sharepointonline",
      "connectionName": "shared_sharepointonline",
      "operationId": "PatchItem"
    },
    "parameters": {
      "dataset": "<SITE>",
      "table": "<LIST>",
      "id": "@triggerBody()?['ID']",
      "item/Status": "Done"
    }
  }
}
```

### Delete item

`pa/Delete_item.json`

```json
{
  "type": "OpenApiConnection",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_sharepointonline",
      "connectionName": "shared_sharepointonline",
      "operationId": "DeleteItem"
    },
    "parameters": {
      "dataset": "<SITE>",
      "table": "<LIST>",
      "id": "@triggerBody()?['ID']"
    }
  }
}
```

### Column keys

The connector reference calls the item a single dynamic parameter. In a flow
definition it is flattened: one `item/<Column>` key per column being written,
and only the columns being written. `<Column>` is the column's **internal**
name, not its display name — a column created as "Due Date" is `DueDate`
forever, and one renamed later keeps whatever internal name it was born with.
A user reads the internal name off **List settings → the column → the `Field=`
value in the address bar**.

A Choice column takes a nested value key, `item/Status/Value`. Person and
Lookup columns take a different key again, and it is worth checking rather than
guessing: ask the user for a peek at any existing flow that writes the column,
or write the text columns first and add the awkward one once its key is known.

## Outlook — `shared_office365`

### When a new email arrives (V3)

`pa/When_a_new_email_arrives.trigger.json`

```json
{
  "type": "OpenApiConnectionNotification",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_office365",
      "connectionName": "shared_office365",
      "operationId": "OnNewEmailV3"
    },
    "parameters": {
      "folderPath": "Inbox",
      "importance": "Any",
      "fetchOnlyWithAttachment": false,
      "includeAttachments": false
    }
  },
  "splitOn": "@triggerOutputs()?['body/value']"
}
```

Note the type: this is the push variant, not `OpenApiConnection`. `subjectFilter`
and `from` narrow it further, and `fetchOnlyUnread` is available too.

### Send an email (V2)

`pa/Send_email.json`

```json
{
  "type": "OpenApiConnection",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_office365",
      "connectionName": "shared_office365",
      "operationId": "SendEmailV2"
    },
    "parameters": {
      "emailMessage/To": "<RECIPIENT>",
      "emailMessage/Subject": "@variables('subject')",
      "emailMessage/Body": "<p>@{variables('summary')}</p>",
      "emailMessage/Importance": "Normal"
    }
  }
}
```

The body is HTML. `emailMessage/Cc`, `/Bcc`, `/ReplyTo` and `/Attachments` are
the other keys worth knowing. Mail goes out as the owner of the connection,
which is one more reason the human binds it at import.

## Teams — `shared_teams`

### Post a message in a channel

`pa/Post_to_channel.json`

```json
{
  "type": "OpenApiConnection",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_teams",
      "connectionName": "shared_teams",
      "operationId": "PostMessageToConversation"
    },
    "parameters": {
      "poster": "Flow bot",
      "location": "Channel",
      "body/recipient/groupId": "<TEAM_ID>",
      "body/recipient/channelId": "<CHANNEL_ID>",
      "body/messageBody": "<p>@{variables('summary')}</p>"
    }
  }
}
```

`poster` is `"Flow bot"` or `"User"`. Posting as `"User"` posts as whoever owns
the connection.

### Message one person

`pa/Notify_owner.json`

```json
{
  "type": "OpenApiConnection",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_teams",
      "connectionName": "shared_teams",
      "operationId": "PostMessageToConversation"
    },
    "parameters": {
      "poster": "Flow bot",
      "location": "Chat with Flow bot",
      "body/recipient": "<UPN>",
      "body/messageBody": "<p>@{variables('summary')}</p>"
    }
  }
}
```

The chat form takes `body/recipient` as a flat value — a user principal name,
or several separated by semicolons — where the channel form takes the nested
`groupId` and `channelId` pair. `body/messageBody` is HTML in both.

## Microsoft Forms — `shared_microsoftforms`

### When a new response is submitted

`pa/When_a_new_response_is_submitted.trigger.json`

```json
{
  "type": "OpenApiConnectionWebhook",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_microsoftforms",
      "connectionName": "shared_microsoftforms",
      "operationId": "CreateFormWebhook"
    },
    "parameters": {
      "form_id": "<FORM_ID>"
    }
  },
  "splitOn": "@triggerOutputs()?['body/value']"
}
```

The trigger hands over a response id and nothing else. Getting the answers takes
a second action.

### Get response details

`pa/Get_response_details.json`

```json
{
  "type": "OpenApiConnection",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_microsoftforms",
      "connectionName": "shared_microsoftforms",
      "operationId": "GetFormResponseById"
    },
    "parameters": {
      "form_id": "<FORM_ID>",
      "response_id": "@triggerOutputs()?['body/resourceData/responseId']"
    }
  }
}
```

Answers come back keyed by question id, not by question text, so a flow that
reads a specific answer needs the ids from a first run or from the user.

## Approvals — `shared_approvals`

### Start and wait for an approval

`pa/Await_approval.json`

```json
{
  "type": "OpenApiConnectionWebhook",
  "inputs": {
    "host": {
      "apiId": "/providers/Microsoft.PowerApps/apis/shared_approvals",
      "connectionName": "shared_approvals",
      "operationId": "StartAndWaitForAnApproval"
    },
    "parameters": {
      "approvalType": "Basic",
      "WebhookApprovalCreationInput/title": "@variables('title')",
      "WebhookApprovalCreationInput/assignedTo": "<APPROVER_UPN>",
      "WebhookApprovalCreationInput/details": "@variables('details')",
      "WebhookApprovalCreationInput/itemLink": "@variables('link')",
      "WebhookApprovalCreationInput/itemLinkDescription": "Open the item",
      "WebhookApprovalCreationInput/enableNotifications": true,
      "WebhookApprovalCreationInput/enableReassignment": true
    }
  }
}
```

The flow parks here until someone responds; the outcome arrives as
`body("Await_approval")?["outcome"]`. `approvalType` is `"Basic"` for
approve-or-reject and `"BasicAwaitAll"` when every assignee must respond. The
parameter prefix is `WebhookApprovalCreationInput/` for this operation and plain
`ApprovalCreationInput/` for `CreateAnApproval`, the fire-and-forget variant
that pairs with a later `WaitForAnApproval`.

## When the operation is not on this page

Microsoft publishes every connector's operations at
`learn.microsoft.com/connectors/<apiname>/` — `sharepointonline`, `office365`,
`teams`, and so on. Each operation lists its Operation ID and a parameter table
whose **Key** column is exactly what goes in `inputs.parameters`. That page
settles two of the three things needed, and the third — the `shared_*` API name
— is the last segment of the URL.

What the reference page will not tell you is how a dynamic parameter flattens.
Where it says `item` or `body` is a single dynamic value, a real flow carries
slash-joined keys underneath it: `item/Title`, `body/recipient/groupId`. The
pattern is consistent, but the leaf names come from the tenant's own columns or
the operation's schema, so infer them from the entries above and confirm rather
than assert. When in doubt ask the user for PA's **Peek code** on one existing
action; that answers it exactly and costs them ten seconds.

## Provenance

Every body on this page was checked against a real flow definition rather than
written from memory. The SharePoint, Outlook, Forms and Approvals shapes were
verified against Microsoft's published connector reference and against exported
flows; the two Teams shapes come from flow definitions Microsoft ships in its
own public sample solutions. Placeholders stand in for every value that would
identify a tenant, and no exported flow, tenant id, site URL or connection id
appears here or should ever be added.

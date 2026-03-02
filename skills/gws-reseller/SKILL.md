---
name: gws-reseller
version: 1.0.0
description: "USE WHEN the user wants to manage workspace subscriptions via the `gws` CLI."
metadata:
  openclaw:
    category: "productivity"
    requires:
      bins: ["gws"]
    cliHelp: "gws reseller --help"
---

# reseller (v1)

> **PREREQUISITE:** Read `../gws-shared/SKILL.md` for auth, global flags, and security rules. If missing, run `gws generate-skills` to create it.

```bash
gws reseller <resource> <method> [flags]
```

## API Resources

### customers

  - `get` — Gets a customer account. Use this operation to see a customer account already in your reseller management, or to see the minimal account information for an existing customer that you do not manage. Fo
  - `insert` — Orders a new customer's account. Before ordering a new customer account, establish whether the customer account already exists using the [`customers.get`](https://developers.google.com/workspace/admin
  - `patch` — Updates a customer account's settings. This method supports patch semantics. You cannot update `customerType` via the Reseller API, but a `"team"` customer can verify their domain and become `customer
  - `update` — Updates a customer account's settings. You cannot update `customerType` via the Reseller API, but a `"team"` customer can verify their domain and become `customerType = "domain"`. For more information

### resellernotify

  - `getwatchdetails` — Returns all the details of the watch corresponding to the reseller.
  - `register` — Registers a Reseller for receiving notifications.
  - `unregister` — Unregisters a Reseller for receiving notifications.

### subscriptions

  - `activate` — Activates a subscription previously suspended by the reseller. If you did not suspend the customer subscription and it is suspended for any other reason, such as for abuse or a pending ToS acceptance,
  - `changePlan` — Updates a subscription plan. Use this method to update a plan for a 30-day trial or a flexible plan subscription to an annual commitment plan with monthly or yearly payments. How a plan is updated dif
  - `changeRenewalSettings` — Updates a user license's renewal settings. This is applicable for accounts with annual commitment plans only. For more information, see the description in [manage subscriptions](https://developers.goo
  - `changeSeats` — Updates a subscription's user license settings. For more information about updating an annual commitment plan or a flexible plan subscription’s licenses, see [Manage Subscriptions](https://developers.
  - `delete` — Cancels, suspends, or transfers a subscription to direct.
  - `get` — Gets a specific subscription. The `subscriptionId` can be found using the [Retrieve all reseller subscriptions](https://developers.google.com/workspace/admin/reseller/v1/how-tos/manage_subscriptions#g
  - `insert` — Creates or transfer a subscription. Create a subscription for a customer's account that you ordered using the [Order a new customer account](https://developers.google.com/workspace/admin/reseller/v1/r
  - `list` — Lists of subscriptions managed by the reseller. The list can be all subscriptions, all of a customer's subscriptions, or all of a customer's transferable subscriptions. Optionally, this method can fil
  - `startPaidService` — Immediately move a 30-day free trial subscription to a paid service subscription. This method is only applicable if a payment plan has already been set up for the 30-day trial subscription. For more i
  - `suspend` — Suspends an active subscription. You can use this method to suspend a paid subscription that is currently in the `ACTIVE` state. * For `FLEXIBLE` subscriptions, billing is paused. * For `ANNUAL_MONTHL

## Discovering Commands

Before calling any API method, inspect it:

```bash
# Browse resources and methods
gws reseller --help

# Inspect a method's required params, types, and defaults
gws schema reseller.<resource>.<method>
```

Use `gws schema` output to build your `--params` and `--json` flags.


# The problem

Machine-paid APIs settle per request today in three different ways, and none of the
three gives the buyer a statement they can check. A transfer happening on-chain says
nothing about what it was for or whether the price charged was the one in force at the
time.

**x402.** The client signs a Soroban authorization entry rather than a transaction; a
facilitator verifies and settles it. Any SEP-41 token works, USDC by default. The
handshake runs over `PAYMENT-REQUIRED`, `PAYMENT-SIGNATURE`, and `PAYMENT-RESPONSE`
headers.

**MPP charge mode.** Each request settles individually as a direct Soroban SAC
transfer. No external facilitator sits in the path.

**MPP session mode.** The funder deposits once into a one-way payment channel
contract, then signs cumulative off-chain commitments as usage accrues. The server
verifies each by simulating `prepare_commitment` and keeps the highest cumulative
amount with its signature. Closing the channel transfers the committed amount and
returns the remainder to the funder.

Session mode has the sharpest failure mode of the three. Revenue earned this way is a
signature sitting in the seller's own database until someone submits it on-chain —
`settle` withdraws against a commitment without closing the channel, but nothing forces
that call to happen before the refund window closes. Once the funder-controlled refund
waiting period elapses, `refund` transfers the entire remaining balance back to the
funder, including amounts the recipient earned but never settled for. The channel
contract reserves nothing for the recipient; the recipient's documented obligation is
to monitor for close events and settle promptly. This contract — `one-way-channel` — is
not audited.

Two other projects sit near this problem. OpenZeppelin operates the Stellar x402
facilitator: `/verify`, `/settle`, `/supported`. It settles payments. RouteDock is a
unified execution layer across x402, MPP charge, and MPP session behind one client
call, with a durable session store and dispute recovery; it routes and executes
requests. Neither reconciles a bill against a price after the fact. Tallybook does that
one job and nothing else.

This is early tooling for a small market. The population of APIs charging machines per
request, and the population of agents paying them, are both still small. Nothing here
assumes that changes on a particular timeline.

Both contracts are live on testnet today: `price_book` at
[`CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6`](https://stellar.expert/explorer/testnet/contract/CC6VXCEIXGGFIZXH2DGPVPUXYVVI3YVEQQ5M7XAWNEN7SADIYACTRFZ6)
and `statement_registry` at
[`CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3`](https://stellar.expert/explorer/testnet/contract/CCT5AAVONSVWQGMINQN7TOJGSUZE6KUIFSSMEN2USNNEZFWR5ZX6UKU3).
See [Contracts](contracts/overview.md) for the full interface.

import { Card, CardHead, PageHeader } from "../../components/ui";

export function Integrations() {
  return (
    <>
      <PageHeader title="Integrations" subtitle="configured in the server's environment" back="/more" />
      <Card className="flex flex-col gap-2 p-4">
        <CardHead title="Bank feeds (Plaid)" />
        <p className="text-muted text-[13px] font-semibold">Set PLAID_CLIENT_ID, PLAID_SECRET and APP_ENCRYPTION_KEY in .env, restart, and the link button appears here. Transactions and balances then sync on a schedule. Landing in a later step.</p>
      </Card>
      <Card className="flex flex-col gap-2 p-4">
        <CardHead title="Virtual cards (Privacy.com)" />
        <p className="text-muted text-[13px] font-semibold">Set PRIVACY_API_KEY to pull card transactions and declines. Landing in a later step.</p>
      </Card>
      <Card className="flex flex-col gap-2 p-4">
        <CardHead title="CSV import" />
        <p className="text-muted text-[13px] font-semibold">Upload a bank export, map the columns once per bank, and Hearth dedupes against what it already has. Landing in a later step.</p>
      </Card>
    </>
  );
}

import { LegalPageLayout, LegalSection, Placeholder } from './LegalPageLayout'

/**
 * Not legal advice — drafted from what the Agon codebase actually collects
 * and processes (see the data-sources list in the PR description) and a
 * standard GDPR-oriented structure, for James to review/have reviewed before
 * relying on it for the Google OAuth branding verification or the Garmin
 * Connect IQ store listing. `Placeholder`-flagged values are assumed, not
 * confirmed — see the PR description.
 */
export function PrivacyPolicyPage() {
  return (
    <LegalPageLayout title="Privacy Policy" updated="23 November 2026">
      <p>
        This Privacy Policy explains what personal data Agon ("Agon", "we", "us")
        collects when you use the Agon app and website (together, the
        "Service"), why we collect it, and what rights you have over it.
      </p>

      <LegalSection title="1. Who we are">
        <p>
          Agon is operated by <Placeholder>[legal entity name — e.g. "James Elgar, trading as Agon," or a registered company name]</Placeholder>,
          based in the United Kingdom. For any question about this policy or
          your data, contact{' '}
          <Placeholder>[privacy contact email, e.g. privacy@get-agon.com]</Placeholder>.
        </p>
      </LegalSection>

      <LegalSection title="2. Data we collect">
        <p>We collect the following categories of personal data:</p>
        <ul className="list-disc space-y-2 pl-5">
          <li>
            <strong>Account data.</strong> When you sign up, we create an
            account using your email address and a password, or via "Sign in
            with Google". If you use Google, we receive your name, email
            address and profile photo from Google to create your Agon
            profile. We don't receive your Google password.
          </li>
          <li>
            <strong>Profile data.</strong> Your display name, profile photo,
            and any other details you add to your Agon profile.
          </li>
          <li>
            <strong>Match &amp; activity data.</strong> The matches, scores,
            teams, tournaments and series you create or take part in; your
            roster memberships, invites, RSVPs and roles; follows; likes and
            comments; and the stats that are calculated from this activity
            (e.g. wins, losses, per-sport stats).
          </li>
          <li>
            <strong>Images you upload.</strong> Profile photos, team crests,
            and match photos you choose to upload.
          </li>
          <li>
            <strong>Garmin watch app data.</strong> If you pair a Garmin
            device running the Agon Connect IQ app, we receive the pairing
            information needed to link your device to your account, and
            whatever match data you use the watch app to record or stream
            (e.g. live scoring input, and if enabled, in‑match metrics such as
            heart rate).
          </li>
          <li>
            <strong>YouTube connection (optional).</strong> If you choose to
            connect your own YouTube channel to record matches, Google asks
            you to grant Agon the "manage your YouTube account" permission
            and we store the resulting OAuth token. We only use it to upload
            match recordings to <em>your</em> channel on your behalf; videos
            are uploaded unlisted by default (visibility is in your control
            once uploaded).
          </li>
          <li>
            <strong>Analytics data.</strong> We use PostHog (hosted in the
            EU) in cookieless mode to understand how the app is used — it
            doesn't use cookies or browser storage, and sessions are only
            tied to your Agon account id once you're signed in, not to any
            other identifier.
          </li>
          <li>
            <strong>Technical/log data.</strong> Standard request and error
            logs/traces/metrics from our servers, used for debugging and
            reliability, not for tracking individual users.
          </li>
        </ul>
      </LegalSection>

      <LegalSection title="3. How we use your data">
        <ul className="list-disc space-y-2 pl-5">
          <li>To create and operate your account and show you the Service.</li>
          <li>
            To run core features: matches, teams, tournaments, stats,
            notifications, the activity feed, and live scoring.
          </li>
          <li>
            To let you connect optional integrations you choose to use
            (Google sign-in, YouTube, a Garmin watch).
          </li>
          <li>To keep the Service secure and investigate abuse.</li>
          <li>
            To understand overall usage of the Service and improve it
            (aggregated analytics — see above).
          </li>
          <li>To contact you about your account or significant changes to the Service.</li>
        </ul>
      </LegalSection>

      <LegalSection title="4. Who we share data with">
        <p>
          We don't sell your data. We share it only with the service
          providers that help us run Agon, and only as needed to provide the
          Service:
        </p>
        <ul className="list-disc space-y-2 pl-5">
          <li>
            <strong>Supabase</strong> — authentication (sign-in/sign-up,
            password handling, Google OAuth).
          </li>
          <li>
            <strong>Google</strong> — if you sign in with Google or connect
            YouTube, per their own terms for that integration.
          </li>
          <li>
            <strong>Amazon Web Services (AWS)</strong> — our primary
            database (DynamoDB) and file storage for uploaded images.
          </li>
          <li>
            <strong>OpenSearch</strong> — stores match and player statistics
            to power leaderboards and summaries.
          </li>
          <li>
            <strong>Google Cloud Platform (GCP)</strong> — hosts the
            application servers.
          </li>
          <li>
            <strong>PostHog</strong> — analytics, as described above (EU
            hosted).
          </li>
        </ul>
        <p>
          We may also disclose data if required by law, or to protect the
          rights, safety or property of Agon, our users, or others.
        </p>
      </LegalSection>

      <LegalSection title="5. Where your data is stored">
        <p>
          Our infrastructure runs on AWS and GCP. We aim to keep data in the
          UK/EU where practical; where a provider processes data outside the
          UK/EU, we rely on that provider's standard contractual clauses or
          equivalent safeguards.
        </p>
      </LegalSection>

      <LegalSection title="6. Data retention">
        <p>
          We keep your account and match data for as long as your account is
          active, since matches and stats are shared with other players and
          teams. If you delete your account, we delete or anonymise your
          personal data within a reasonable period, except where we need to
          keep limited records for legal, security or dispute-resolution
          purposes, or where data has become part of another user's own
          match/team records (e.g. a shared match result).
        </p>
      </LegalSection>

      <LegalSection title="7. Your rights (UK GDPR)">
        <p>Under UK data protection law, you have the right to:</p>
        <ul className="list-disc space-y-2 pl-5">
          <li>Access the personal data we hold about you.</li>
          <li>Ask us to correct inaccurate data.</li>
          <li>Ask us to delete your data ("right to be forgotten").</li>
          <li>Ask us to restrict or object to certain processing.</li>
          <li>Receive your data in a portable format.</li>
          <li>
            Complain to the UK Information Commissioner's Office (ICO) at{' '}
            <a
              href="https://ico.org.uk"
              target="_blank"
              rel="noreferrer"
              className="underline"
            >
              ico.org.uk
            </a>{' '}
            if you believe we haven't handled your data properly.
          </li>
        </ul>
        <p>
          To exercise any of these rights, email{' '}
          <Placeholder>[privacy contact email]</Placeholder>. You can also
          delete most of your own content directly in the app, and request
          full account deletion by contacting us.
        </p>
      </LegalSection>

      <LegalSection title="8. Children">
        <p>
          Agon isn't directed at children, and we don't knowingly collect
          data from children under 13. If you believe a child has created an
          account, contact us and we'll remove it.
        </p>
      </LegalSection>

      <LegalSection title="9. Security">
        <p>
          We use industry-standard measures (encryption in transit, access
          controls on our infrastructure) to protect your data, but no
          service can guarantee perfect security.
        </p>
      </LegalSection>

      <LegalSection title="10. Changes to this policy">
        <p>
          We may update this policy as the Service changes. We'll update the
          "Last updated" date above, and for material changes we'll make a
          reasonable effort to notify you in the app.
        </p>
      </LegalSection>

      <LegalSection title="11. Contact us">
        <p>
          Questions about this policy or your data:{' '}
          <Placeholder>[privacy contact email]</Placeholder>.
        </p>
      </LegalSection>

      <p className="pt-4 text-xs text-muted-foreground">
        This document is a draft prepared for review and isn't legal advice.
        It should be checked (ideally by a solicitor or data protection
        professional) before being relied on for regulatory purposes, such as
        Google OAuth branding verification.
      </p>
    </LegalPageLayout>
  )
}

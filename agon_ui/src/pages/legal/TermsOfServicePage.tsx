import { LegalPageLayout, LegalSection, Placeholder } from './LegalPageLayout'

/**
 * Not legal advice — see the module doc on `PrivacyPolicyPage` for the same
 * caveat. `Placeholder`-flagged values are assumed, not confirmed — see the
 * PR description.
 */
export function TermsOfServicePage() {
  return (
    <LegalPageLayout title="Terms of Service" updated="23 November 2026">
      <p>
        These Terms of Service ("Terms") govern your use of the Agon app and
        website (the "Service"), operated by{' '}
        <Placeholder>[legal entity name]</Placeholder> ("Agon", "we", "us").
        By creating an account or using the Service, you agree to these
        Terms and to our{' '}
        <a href="/privacy" className="underline">
          Privacy Policy
        </a>
        .
      </p>

      <LegalSection title="1. The Service">
        <p>
          Agon lets you organise and track matches, teams and tournaments
          across sports, follow other players, view stats, and (optionally)
          connect a Garmin watch app or your own YouTube channel to enrich
          that activity. We may add, change or remove features over time.
        </p>
      </LegalSection>

      <LegalSection title="2. Accounts">
        <ul className="list-disc space-y-2 pl-5">
          <li>
            You need an account to use most of the Service, created with an
            email address and password or via Google sign-in.
          </li>
          <li>
            You're responsible for keeping your account credentials secure
            and for activity that happens under your account.
          </li>
          <li>
            You must give accurate information and keep it up to date.
          </li>
          <li>You must be old enough to form a binding contract in your country to create an account.</li>
        </ul>
      </LegalSection>

      <LegalSection title="3. Your content">
        <p>
          You keep ownership of the content you add to Agon — profile
          photos, match details, comments, and any videos you upload via the
          YouTube integration. By posting content, you grant Agon a licence
          to store, display and distribute it within the Service (for
          example, showing a match to the other players in it, or on a
          public profile/team page you've chosen to make visible). You're
          responsible for making sure you have the right to post what you
          upload, and for respecting other players' privacy (e.g. don't
          upload a match recording that violates someone else's rights).
        </p>
      </LegalSection>

      <LegalSection title="4. Acceptable use">
        <p>You agree not to:</p>
        <ul className="list-disc space-y-2 pl-5">
          <li>Use the Service for anything unlawful, or to harass, abuse or impersonate others.</li>
          <li>Try to access another user's account or data without permission.</li>
          <li>Interfere with the Service's normal operation (e.g. scraping, overloading, reverse-engineering it to bypass access controls).</li>
          <li>Upload content you don't have the rights to, including via the YouTube integration.</li>
        </ul>
        <p>We may suspend or terminate accounts that violate these Terms.</p>
      </LegalSection>

      <LegalSection title="5. Third-party integrations">
        <p>
          Some features connect to third-party services on your behalf:
          Google (sign-in and, if you choose, YouTube uploads to your own
          channel) and Garmin Connect IQ (if you pair a watch). Your use of
          those integrations is also subject to that provider's own terms.
          We aren't responsible for the availability or behaviour of
          third-party services.
        </p>
      </LegalSection>

      <LegalSection title="6. Disclaimers">
        <p>
          The Service is provided "as is". We don't guarantee it will always
          be available, error-free, or suitable for a particular purpose
          (for example, stats and scores are only as accurate as the data
          players enter). Nothing in these Terms limits any right you have
          under law that can't be excluded.
        </p>
      </LegalSection>

      <LegalSection title="7. Limitation of liability">
        <p>
          To the extent permitted by law, Agon isn't liable for indirect or
          consequential losses arising from your use of the Service. Nothing
          here excludes liability that can't be excluded under applicable
          law.
        </p>
      </LegalSection>

      <LegalSection title="8. Ending your use">
        <p>
          You can stop using the Service, and delete your account, at any
          time (contact{' '}
          <Placeholder>[support/privacy contact email]</Placeholder> for
          account deletion). We may suspend or terminate access for
          violation of these Terms.
        </p>
      </LegalSection>

      <LegalSection title="9. Changes to these Terms">
        <p>
          We may update these Terms as the Service changes. We'll update the
          "Last updated" date above, and for material changes we'll make a
          reasonable effort to notify you in the app. Continuing to use the
          Service after a change means you accept the updated Terms.
        </p>
      </LegalSection>

      <LegalSection title="10. Governing law">
        <p>
          These Terms are governed by the laws of{' '}
          <Placeholder>[governing law/jurisdiction — assumed England and Wales, given the operator is UK-based]</Placeholder>.
        </p>
      </LegalSection>

      <LegalSection title="11. Contact us">
        <p>
          Questions about these Terms:{' '}
          <Placeholder>[contact email]</Placeholder>.
        </p>
      </LegalSection>

      <p className="pt-4 text-xs text-muted-foreground">
        This document is a draft prepared for review and isn't legal advice.
        It should be checked (ideally by a solicitor) before being relied on
        for regulatory purposes, such as Google OAuth branding verification.
      </p>
    </LegalPageLayout>
  )
}

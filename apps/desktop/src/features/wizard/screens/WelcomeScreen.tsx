/** Screen 1 — Welcome (spec §4.1). */
export function WelcomeScreen() {
  return (
    <section className="screen screen--center" aria-labelledby="welcome-title">
      <p className="eyebrow">Hotwire</p>
      <h1 className="screen-title screen-title--display" id="welcome-title">
        Your keyboard has more buttons
        <br />
        than your workflow needs.
      </h1>
      <p className="screen-lede">Let&rsquo;s give them better jobs.</p>
      <p className="screen-lede muted">
        Hotwire runs locally and only intercepts the keys you assign.
      </p>
    </section>
  );
}

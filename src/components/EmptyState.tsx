export default function EmptyState({ tracking }: { tracking: boolean }) {
  return (
    <div className="empty">
      <div className="big">Nothing on the receipt yet</div>
      <p>
        Trace records in the background — there is nothing to start and nothing to press.
        Keep using your computer and this day fills itself in: apps, sites, focus, and a verdict.
        {tracking ? ' Tracking is on right now.' : ' Tracking is paused — turn it back on from the header to resume.'}
      </p>
    </div>
  );
}

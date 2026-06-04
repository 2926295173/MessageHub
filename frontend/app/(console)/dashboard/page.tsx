export default function DashboardPage() {
  return (
    <div className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold">Dashboard</h1>
        <p className="text-sm text-base-content/60">
          Status of all paired Android devices.
        </p>
      </header>

      <section className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-4">
        <div className="card bg-base-200">
          <div className="card-body">
            <div className="text-sm text-base-content/60">Online devices</div>
            <div className="text-3xl font-semibold">—</div>
          </div>
        </div>
        <div className="card bg-base-200">
          <div className="card-body">
            <div className="text-sm text-base-content/60">Unread notifications</div>
            <div className="text-3xl font-semibold">—</div>
          </div>
        </div>
        <div className="card bg-base-200">
          <div className="card-body">
            <div className="text-sm text-base-content/60">Unread SMS</div>
            <div className="text-3xl font-semibold">—</div>
          </div>
        </div>
        <div className="card bg-base-200">
          <div className="card-body">
            <div className="text-sm text-base-content/60">Recent calls (24h)</div>
            <div className="text-3xl font-semibold">—</div>
          </div>
        </div>
      </section>

      <section className="card bg-base-200">
        <div className="card-body">
          <h2 className="card-title">Welcome</h2>
          <p className="text-sm">
            PhoneBridge web console is wired up. Real data will appear once the
            daemon exposes REST endpoints in M1 and the WebSocket client is
            connected in M8.
          </p>
        </div>
      </section>
    </div>
  );
}

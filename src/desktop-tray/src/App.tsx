import { invoke } from "@tauri-apps/api/core";

function App() {
  const handlePing = async () => {
    try {
      const msg = await invoke<string>("ping");
      console.log(msg);
    } catch (e) {
      console.error(e);
    }
  };

  return (
    <div className="min-w-[280px] p-4 bg-zinc-900 text-zinc-100 rounded-lg">
      <h1 className="text-lg font-semibold">VibeAround</h1>
      <p className="text-sm text-zinc-400 mt-1">Tray control panel</p>
      <button
        type="button"
        onClick={handlePing}
        className="mt-3 px-3 py-1.5 bg-zinc-700 rounded text-sm hover:bg-zinc-600"
      >
        Ping backend
      </button>
    </div>
  );
}

export default App;

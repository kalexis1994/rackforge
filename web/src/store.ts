import { configureStore, createSlice, type PayloadAction } from "@reduxjs/toolkit";
import type {
  ConnectionStatus,
  PerformanceSnapshot,
  SessionSnapshot,
} from "./types";

interface RackForgeState {
  connection: ConnectionStatus;
  snapshot: SessionSnapshot | null;
  performance: PerformanceSnapshot | null;
  performancePending: boolean;
  error: string | null;
}

const initialState: RackForgeState = {
  connection: "connecting",
  snapshot: null,
  performance: null,
  performancePending: false,
  error: null,
};

const rackForgeSlice = createSlice({
  name: "rackforge",
  initialState,
  reducers: {
    connectionChanged(state, action: PayloadAction<ConnectionStatus>) {
      state.connection = action.payload;
      // A reconnect attempt is not a recovery. Keep a confirmed persistent
      // outage visible until the transport actually reaches ONLINE again.
      if (action.payload === "online") {
        state.error = null;
      }
    },
    snapshotReceived(state, action: PayloadAction<SessionSnapshot>) {
      if (state.snapshot?.revision !== action.payload.revision) {
        state.snapshot = action.payload;
      }
      state.connection = "online";
      state.error = null;
    },
    hostIdleReceived(state) {
      state.snapshot = null;
      state.performance = null;
      state.performancePending = false;
      state.connection = "idle";
      state.error = null;
    },
    performanceReceived(
      state,
      action: PayloadAction<{ snapshot: PerformanceSnapshot; edited: boolean }>,
    ) {
      state.performance = action.payload.snapshot;
      if (action.payload.edited) state.performancePending = false;
      state.connection = "online";
      state.error = null;
    },
    performanceEditStarted(state) {
      state.performancePending = true;
      state.error = null;
    },
    errorReceived(state, action: PayloadAction<string>) {
      state.error = action.payload;
      state.performancePending = false;
    },
  },
});

export const {
  connectionChanged,
  hostIdleReceived,
  snapshotReceived,
  performanceReceived,
  performanceEditStarted,
  errorReceived,
} = rackForgeSlice.actions;

export const store = configureStore({
  reducer: {
    rackforge: rackForgeSlice.reducer,
  },
});

export type RootState = ReturnType<typeof store.getState>;

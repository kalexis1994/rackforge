import { configureStore, createSlice, type PayloadAction } from "@reduxjs/toolkit";
import type { ConnectionStatus, SessionSnapshot } from "./types";

interface RackForgeState {
  connection: ConnectionStatus;
  snapshot: SessionSnapshot | null;
  error: string | null;
}

const initialState: RackForgeState = {
  connection: "connecting",
  snapshot: null,
  error: null,
};

const rackForgeSlice = createSlice({
  name: "rackforge",
  initialState,
  reducers: {
    connectionChanged(state, action: PayloadAction<ConnectionStatus>) {
      state.connection = action.payload;
      if (action.payload === "online") state.error = null;
    },
    snapshotReceived(state, action: PayloadAction<SessionSnapshot>) {
      if (state.snapshot?.revision !== action.payload.revision) {
        state.snapshot = action.payload;
      }
      state.connection = "online";
      state.error = null;
    },
    errorReceived(state, action: PayloadAction<string>) {
      state.error = action.payload;
    },
  },
});

export const { connectionChanged, snapshotReceived, errorReceived } =
  rackForgeSlice.actions;

export const store = configureStore({
  reducer: {
    rackforge: rackForgeSlice.reducer,
  },
});

export type RootState = ReturnType<typeof store.getState>;

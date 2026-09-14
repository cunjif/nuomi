import { useCallback, useRef, useState } from "react";
import { ipc } from "../../../lib/ipc/client";
import { toast } from "../../../lib/store/toastStore";
import { useTranslation } from "react-i18next";
import { describeError } from "../../../i18n";

export type VoiceState = "idle" | "capturing" | "transcribing";

export interface UseVoiceInputResult {
  state: VoiceState;
  toggle: () => void;
  /** Last transcribed text; cleared after consumed. */
  text: string | null;
  consumeText: () => void;
}

/**
 * Voice capture + transcription hook. Uses MediaRecorder for capture,
 * `ipc.transcribeAudio` for transcription. Only activates on user action
 * (never auto-starts the microphone).
 */
export function useVoiceInput(): UseVoiceInputResult {
  const { t } = useTranslation();
  const [state, setState] = useState<VoiceState>("idle");
  const [text, setText] = useState<string | null>(null);
  const recorderRef = useRef<MediaRecorder | null>(null);
  const chunksRef = useRef<Blob[]>([]);

  const stop = useCallback((): void => {
    const recorder = recorderRef.current;
    if (recorder && recorder.state !== "inactive") {
      recorder.stop();
    }
    recorderRef.current = null;
    chunksRef.current = [];
  }, []);

  const transcribe = useCallback(async (audioBlob: Blob): Promise<void> => {
    setState("transcribing");
    try {
      const reader = new FileReader();
      reader.onload = async (): Promise<void> => {
        const result = reader.result;
        if (typeof result !== "string") {
          setState("idle");
          return;
        }
        const base64 = result.split(",")[1] ?? "";
        try {
          const transcribed = await ipc.transcribeAudio(base64, null);
          if (transcribed.length > 0) {
            setText(transcribed);
          } else {
            toast.error(t("composer.voice.emptyResult"));
          }
        } catch (e) {
          toast.error(t("composer.voice.failed", { message: describeError(e) }));
        }
        setState("idle");
      };
      reader.readAsDataURL(audioBlob);
    } catch (e) {
      toast.error(t("composer.voice.failed", { message: describeError(e) }));
      setState("idle");
    }
  }, [t]);

  const toggle = useCallback((): void => {
    if (state === "idle") {
      chunksRef.current = [];
      navigator.mediaDevices
        .getUserMedia({ audio: true })
        .then((stream) => {
          const recorder = new MediaRecorder(stream);
          recorder.ondataavailable = (e): void => {
            if (e.data.size > 0) chunksRef.current.push(e.data);
          };
          recorder.onstop = (): void => {
            stream.getTracks().forEach((track) => track.stop());
            const blob = new Blob(chunksRef.current, { type: "audio/webm" });
            void transcribe(blob);
          };
          recorder.start();
          recorderRef.current = recorder;
          setState("capturing");
        })
        .catch(() => {
          toast.error(t("composer.voice.micDenied"));
          setState("idle");
        });
    } else if (state === "capturing") {
      stop();
    }
  }, [state, stop, transcribe, t]);

  const consumeText = useCallback((): void => {
    setText(null);
  }, []);

  return { state, toggle, text, consumeText };
}

"use client";

import {
  Fragment,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useDebounceValue } from "usehooks-ts";

const checkboxSize = 32;
const checkboxPadding = 4;

type ListState = {
  [key: number]: boolean;
};

export function CheckboxList() {
  const webSocket = useWebSocket();
  const listState = useRef({} as ListState);
  const [changesCount, setChangesCount] = useState(0);
  const parentRef = useRef(null);
  const windowSize = useWindowSize();
  const width = windowSize.width ?? 1360;
  const columnLength = Math.floor(width / (checkboxSize + checkboxPadding));

  const [rangeStart, updateRangeStart] = useDebounceValue(0, 1000);
  const [rangeEnd, updateRangeEnd] = useDebounceValue(1000, 1000);

  const rowVirtualizer = useVirtualizer({
    count: zeroToTenMilion.length / columnLength,
    getScrollElement: () => parentRef.current,
    estimateSize: () => checkboxSize + checkboxPadding,
    overscan: 10,
    onChange: (rowVirtualizer) => {
      const range = {
        start: (rowVirtualizer.range?.startIndex ?? 0) * columnLength,
        end:
          (rowVirtualizer.range?.endIndex ?? 0) * columnLength + columnLength,
      };

      updateRangeStart(range.start);
      updateRangeEnd(range.end);
    },
  });

  const columnVirtualizer = useVirtualizer({
    horizontal: true,
    count: columnLength,
    getScrollElement: () => parentRef.current,
    estimateSize: () => checkboxSize + checkboxPadding,
    overscan: 10,
  });

  const rowsVirtual = rowVirtualizer.getVirtualItems();
  const columnsVirtual = columnVirtualizer.getVirtualItems();

  useEffect(() => {
    if (!webSocket.connected) {
      return;
    }

    webSocket.send(`get,${rangeStart},${rangeEnd}`);
  }, [webSocket.connected, rangeStart, rangeEnd]);

  useEffect(() => {
    const cleanup = webSocket.onMessage((event) => {
      if (event.data.includes("get,")) {
        const data = event.data.split(",");
        data.shift();

        console.log(data);

        // tuples are of type "(index:checked)" where checked is 0 or 1
        data.forEach((tuples: `${string}:${string}`) => {
          const [index, checked] = tuples.split(":");
          if (!index || !checked) {
            return;
          }

          if (checked === "1") {
            listState.current[Number(index)] = true;
          } else {
            delete listState.current[Number(index)];
          }
        });

        setChangesCount((prev) => prev + 1);
        return;
      }

      const [type, index] = event.data.split(",");

      if (type === "c") {
        listState.current[index] = true;
      }
      if (type === "u") {
        delete listState.current[index];
      }

      setChangesCount((prev) => prev + 1);
    });

    return cleanup;
  }, [webSocket]);

  return (
    <>
      <div
        ref={parentRef}
        style={{
          height: "calc(100vh - 160px)",
          width: (checkboxSize + checkboxPadding) * columnLength + "px",
          overflow: "auto",
        }}
      >
        <div
          style={{
            position: "relative",
            height: `${rowVirtualizer.getTotalSize()}px`,
            width: `${columnVirtualizer.getTotalSize()}px`,
          }}
        >
          {rowsVirtual.map((virtualRow) => (
            <Fragment key={virtualRow.key}>
              {columnsVirtual.map((virtualColumn) => {
                const actualIndex =
                  virtualRow.index * columnLength + virtualColumn.index;

                return (
                  <div
                    key={virtualColumn.key}
                    className="absolute top-0 left-0 flex items-center justify-center"
                    style={{
                      width: `${virtualColumn.size}px`,
                      height: `${virtualRow.size}px`,
                      transform: `translateX(${virtualColumn.start}px) translateY(${virtualRow.start}px)`,
                    }}
                  >
                    <input
                      id={`${virtualRow.index},${virtualColumn.index}`}
                      type="checkbox"
                      className={`h-8 w-8`}
                      checked={listState.current[actualIndex] ?? false}
                      onChange={(e) => {
                        if (e.currentTarget.checked) {
                          listState.current[actualIndex] = true;
                        } else {
                          delete listState.current[actualIndex];
                        }
                        setChangesCount((prev) => prev + 1);
                        webSocket.send(
                          `${e.currentTarget.checked ? "c" : "u"},${actualIndex}`
                        );
                      }}
                    />
                  </div>
                );
              })}
            </Fragment>
          ))}
        </div>
      </div>
    </>
  );
}

const zeroToTenMilion = Array.from({ length: 10_000_000 }, (_, i) =>
  i.toString()
);

function useWebSocket() {
  const ws = useRef<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);

  useEffectOnlyOnce(() => {
    ws.current = new WebSocket("wss://web-server-2.fly.dev/");
    const onOpen = () => {
      setConnected(true);
    };

    ws.current.addEventListener("open", onOpen);

    return () => {
      ws.current?.removeEventListener("open", onOpen);
    };
  });

  return useMemo(
    () => ({
      connected,
      send: (data: any) => {
        if (!connected || ws.current?.readyState !== WebSocket.OPEN) {
          return;
        }

        ws.current?.send(data);
      },
      onMessage: (callback: (event: MessageEvent) => void) => {
        if (!connected) {
          return;
        }

        ws.current?.addEventListener("message", callback);

        return () => {
          ws.current?.removeEventListener("message", callback);
        };
      },
    }),
    [connected]
  );
}

function useEffectOnlyOnce(callback: () => void) {
  const hasRun = useRef(false);

  useEffect(() => {
    if (!hasRun.current) {
      callback();
    }
  }, []);
}

export function useWindowSize() {
  const [size, setSize] = useState<{
    width: number | null;
    height: number | null;
  }>({
    width: null,
    height: null,
  });

  useLayoutEffect(() => {
    const handleResize = () => {
      setSize({
        width: window.innerWidth,
        height: window.innerHeight,
      });
    };

    handleResize();
    window.addEventListener("resize", handleResize);

    return () => {
      window.removeEventListener("resize", handleResize);
    };
  }, []);

  useEffect(() => {
    const timerId = setTimeout(() => {
      setSize({
        width: window.innerWidth,
        height: window.innerHeight,
      });
    }, 0);

    return () => {
      clearTimeout(timerId);
    };
  }, []);

  return size;
}

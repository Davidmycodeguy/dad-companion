import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { useNavigate } from "react-router";

import { ItemGroupSummary } from "@/components/item-group-summary";
import { Command, CommandDialog, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import type { ItemGroup } from "@/lib/api";
import { useItemSearch } from "@/lib/queries";

const SearchContext = createContext<() => void>(() => {});

/** Opens the item search (also Ctrl+K anywhere). */
export function useOpenItemSearch() {
  return useContext(SearchContext);
}

export function ItemSearchProvider({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const openSearch = useCallback(() => setOpen(true), []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setOpen((value) => !value);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <SearchContext.Provider value={openSearch}>
      {children}
      <ItemSearchDialog open={open} onOpenChange={setOpen} />
    </SearchContext.Provider>
  );
}

function ItemSearchDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const [query, setQuery] = useState("");
  const navigate = useNavigate();
  const { data: groups = [], isFetching } = useItemSearch(open ? query : "");
  const trimmed = query.trim();
  const shown = trimmed ? groups : [];

  const choose = (name: string) => {
    onOpenChange(false);
    setQuery("");
    navigate(`/market/${encodeURIComponent(name)}`);
  };

  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpenChange}
      title="Search items"
      description="Find any item by name"
      className="top-[12%] max-w-xl"
    >
      <Command shouldFilter={false} loop>
        <CommandInput placeholder="Search items…" value={query} onValueChange={setQuery} />
        <CommandList className="max-h-[min(60vh,28rem)]">
          <CommandEmpty>
            {!trimmed
              ? "Type an item name, e.g. “arming sword”."
              : isFetching
                ? "Searching…"
                : `No item is called “${trimmed}”.`}
          </CommandEmpty>
          {shown.length > 0 && (
            <CommandGroup heading="Items">
              {shown.map((group) => (
                <SearchRow key={group.name} group={group} onChoose={choose} />
              ))}
            </CommandGroup>
          )}
        </CommandList>
      </Command>
    </CommandDialog>
  );
}

function SearchRow({ group, onChoose }: { group: ItemGroup; onChoose: (name: string) => void }) {
  return (
    <CommandItem value={group.name} onSelect={() => onChoose(group.name)} className="gap-3 py-1.5">
      <ItemGroupSummary group={group} slotSize={34} />
    </CommandItem>
  );
}

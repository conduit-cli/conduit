import { useState, useMemo, useRef, useEffect } from 'react';
import { X, Filter, Wrench, MessageSquare, AlertCircle, History, Zap, ChevronDown } from 'lucide-react';
import type { AgentEvent } from '../types';
import { RawEventCard } from './RawEventCard';
import { cn } from '../lib/cn';

type FilterType = 'all' | 'errors' | 'tools' | 'messages' | 'debug' | 'turns';

interface RawEventsPanelProps {
  events: AgentEvent[];
  onClose: () => void;
}

function getEventCategory(event: AgentEvent): FilterType {
  if (event.type === 'Error' || event.type === 'TurnFailed') {
    return 'errors';
  }
  if (event.type === 'ToolStarted' || event.type === 'ToolCompleted') {
    return 'tools';
  }
  if (event.type === 'AssistantMessage' || event.type === 'AssistantReasoning') {
    return 'messages';
  }
  if (event.type === 'TurnStarted' || event.type === 'TurnCompleted' || event.type === 'TokenUsage') {
    return 'turns';
  }
  if (event.type === 'Raw') {
    return 'debug';
  }
  return 'all';
}

function FilterDropdown({
  value,
  onChange,
}: {
  value: FilterType;
  onChange: (value: FilterType) => void;
}) {
  const [isOpen, setIsOpen] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function handleClickOutside(event: MouseEvent) {
      if (dropdownRef.current && !dropdownRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    }
    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  const options: { value: FilterType; label: string; icon: React.ReactNode }[] = [
    { value: 'all', label: 'All Events', icon: <Filter className="h-3.5 w-3.5" /> },
    { value: 'errors', label: 'Errors', icon: <AlertCircle className="h-3.5 w-3.5" /> },
    { value: 'tools', label: 'Tools', icon: <Wrench className="h-3.5 w-3.5" /> },
    { value: 'messages', label: 'Messages', icon: <MessageSquare className="h-3.5 w-3.5" /> },
    { value: 'debug', label: 'Debug', icon: <History className="h-3.5 w-3.5" /> },
    { value: 'turns', label: 'Turns', icon: <Zap className="h-3.5 w-3.5" /> },
  ];

  const selected = options.find((opt) => opt.value === value) ?? options[0];

  return (
    <div ref={dropdownRef} className="relative">
      <button
        onClick={() => setIsOpen(!isOpen)}
        className={cn(
          'flex items-center gap-1.5 rounded-md px-2 py-1 text-xs',
          'bg-surface-elevated text-text-secondary',
          'hover:bg-bg-highlight hover:text-text transition-colors',
          'border border-border'
        )}
      >
        {selected.icon}
        <span>{selected.label}</span>
        <ChevronDown className={cn('h-3 w-3 transition-transform', isOpen && 'rotate-180')} />
      </button>

      {isOpen && (
        <div className="absolute right-0 top-full z-20 mt-1 min-w-[140px] rounded-md border border-border bg-surface-elevated py-1 shadow-lg">
          {options.map((option) => (
            <button
              key={option.value}
              onClick={() => {
                onChange(option.value);
                setIsOpen(false);
              }}
              className={cn(
                'flex w-full items-center gap-2 px-3 py-1.5 text-xs',
                'hover:bg-bg-highlight transition-colors',
                option.value === value ? 'text-accent' : 'text-text-secondary'
              )}
            >
              {option.icon}
              <span>{option.label}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function RawEventsPanel({ events, onClose }: RawEventsPanelProps) {
  const [filter, setFilter] = useState<FilterType>('all');
  const [autoScroll, setAutoScroll] = useState(true);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const prevEventsLength = useRef(events.length);

  const indexedEvents = useMemo(
    () => events.map((event, index) => ({ event, index })),
    [events]
  );

  // Filter events based on selected category
  const filteredEvents = useMemo(() => {
    if (filter === 'all') return indexedEvents;
    return indexedEvents.filter(({ event }) => getEventCategory(event) === filter);
  }, [indexedEvents, filter]);

  // Calculate stats
  const stats = useMemo(() => {
    let errors = 0;
    let tools = 0;
    let messages = 0;

    events.forEach((event) => {
      const category = getEventCategory(event);
      if (category === 'errors') errors++;
      if (category === 'tools') tools++;
      if (category === 'messages') messages++;
    });

    return { total: events.length, errors, tools, messages };
  }, [events]);

  // Scroll to bottom on initial mount
  useEffect(() => {
    if (scrollContainerRef.current) {
      scrollContainerRef.current.scrollTop = scrollContainerRef.current.scrollHeight;
    }
  }, []);

  // Auto-scroll to bottom when new events arrive
  useEffect(() => {
    if (!autoScroll || !scrollContainerRef.current) return;
    if (events.length > prevEventsLength.current) {
      scrollContainerRef.current.scrollTop = scrollContainerRef.current.scrollHeight;
    }
    prevEventsLength.current = events.length;
  }, [events.length, autoScroll]);

  // Detect scroll position for auto-scroll toggle
  useEffect(() => {
    const container = scrollContainerRef.current;
    if (!container) return;

    const handleScroll = () => {
      const distanceFromBottom = container.scrollHeight - (container.scrollTop + container.clientHeight);
      setAutoScroll(distanceFromBottom < 50);
    };

    container.addEventListener('scroll', handleScroll, { passive: true });
    return () => container.removeEventListener('scroll', handleScroll);
  }, []);

  return (
    <div className="absolute right-0 top-0 z-10 flex h-full w-full max-w-lg flex-col border-l border-border bg-surface shadow-xl">
      {/* Header */}
      <div className="flex shrink-0 items-center justify-between gap-3 border-b border-border px-4 py-3">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-text">Raw Events</span>
          <span className="rounded-full bg-accent/15 px-2 py-0.5 text-[10px] font-medium tabular-nums text-accent">
            {filteredEvents.length}
          </span>
        </div>

        <div className="flex items-center gap-2">
          <FilterDropdown value={filter} onChange={setFilter} />

          <button
            onClick={onClose}
            className="rounded-md p-1.5 text-text-muted hover:bg-surface-elevated hover:text-text transition-colors"
            title="Close (Esc)"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
      </div>

      {/* Events list */}
      <div
        ref={scrollContainerRef}
        className="flex-1 overflow-y-auto overflow-x-hidden p-3"
      >
        {filteredEvents.length === 0 ? (
          <div className="flex h-full items-center justify-center">
            <p className="text-xs text-text-muted">
              {events.length === 0 ? 'No events captured yet.' : 'No events match the current filter.'}
            </p>
          </div>
        ) : (
          <div className="space-y-2">
            {filteredEvents.map(({ event, index }) => (
              <RawEventCard key={`event-${index}`} event={event} index={index} />
            ))}
          </div>
        )}
      </div>

      {/* Footer stats bar */}
      <div className="shrink-0 border-t border-border bg-surface-elevated px-4 py-2">
        <div className="flex items-center justify-between text-[10px] text-text-muted">
          <div className="flex items-center gap-3">
            <span>{stats.total} events</span>
            <span className={cn(stats.errors > 0 && 'text-error')}>
              {stats.errors} errors
            </span>
            <span>{stats.tools} tools</span>
          </div>

          <button
            onClick={() => {
              setAutoScroll(true);
              if (scrollContainerRef.current) {
                scrollContainerRef.current.scrollTop = scrollContainerRef.current.scrollHeight;
              }
            }}
            className={cn(
              'rounded px-2 py-0.5 transition-colors',
              autoScroll
                ? 'bg-accent/15 text-accent'
                : 'hover:bg-surface text-text-muted hover:text-text'
            )}
          >
            Auto-scroll {autoScroll ? 'on' : 'off'}
          </button>
        </div>
      </div>
    </div>
  );
}

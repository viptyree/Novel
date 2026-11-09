import { createContext, ReactNode, useContext, useLayoutEffect, useRef } from "react";

interface Activity { dirty: boolean; busy: boolean; }
const ProjectActivity = createContext<Map<string, Activity> | null>(null);

export function ProjectActivityProvider({ children }: { children: ReactNode }) {
  const activities = useRef(new Map<string, Activity>());
  return <ProjectActivity.Provider value={activities.current}>{children}</ProjectActivity.Provider>;
}

export function useProjectActivities() {
  const activities = useContext(ProjectActivity);
  if (!activities) throw new Error("Missing ProjectActivityProvider");
  return activities;
}

export function useProjectActivity(key: string, dirty: boolean, busy: boolean) {
  const activities = useProjectActivities();
  useLayoutEffect(() => {
    activities.set(key, { dirty, busy });
    return () => { activities.delete(key); };
  }, [activities, key, dirty, busy]);
}

import { useEffect, useRef, type MutableRefObject } from 'react'
import * as echarts from 'echarts/core'
import { BarChart, HeatmapChart, LineChart, PieChart } from 'echarts/charts'
import {
  BrushComponent,
  CalendarComponent,
  DataZoomComponent,
  GridComponent,
  LegendComponent,
  TooltipComponent,
  VisualMapComponent,
} from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import type { EChartsCoreOption } from 'echarts/core'

// Tree-shaken ECharts build: only what the graphs page uses.
echarts.use([
  BarChart,
  HeatmapChart,
  LineChart,
  PieChart,
  BrushComponent,
  CalendarComponent,
  DataZoomComponent,
  GridComponent,
  LegendComponent,
  TooltipComponent,
  VisualMapComponent,
  CanvasRenderer,
])

export type { EChartsCoreOption }
export type EChartsInstance = echarts.ECharts

export default function EChart({
  option,
  height = 240,
  onEvents,
  chartRef,
}: {
  option: EChartsCoreOption
  height?: number
  /** ECharts event → handler map (e.g. `brushEnd`). Names are registered once on
   *  mount; handlers themselves are always read fresh, so they can close over state. */
  onEvents?: Record<string, (params: unknown) => void>
  /** Receives the chart instance for imperative calls (dispatchAction etc.). */
  chartRef?: MutableRefObject<EChartsInstance | null>
}) {
  const ref = useRef<HTMLDivElement>(null)
  const chart = useRef<EChartsInstance | null>(null)
  const eventsRef = useRef(onEvents)
  eventsRef.current = onEvents

  useEffect(() => {
    if (!ref.current) return
    const c = echarts.init(ref.current)
    chart.current = c
    if (chartRef) chartRef.current = c
    const ro = new ResizeObserver(() => c.resize())
    ro.observe(ref.current)

    const registered: Array<[string, (p: unknown) => void]> = []
    for (const name of Object.keys(eventsRef.current ?? {})) {
      const handler = (p: unknown) => eventsRef.current?.[name]?.(p)
      registered.push([name, handler])
      c.on(name, handler)
    }

    return () => {
      for (const [name, handler] of registered) c.off(name, handler)
      ro.disconnect()
      c.dispose()
      chart.current = null
      if (chartRef) chartRef.current = null
    }
  }, [chartRef])

  useEffect(() => {
    chart.current?.setOption(option, { notMerge: true })
  }, [option])

  return <div ref={ref} style={{ height }} />
}

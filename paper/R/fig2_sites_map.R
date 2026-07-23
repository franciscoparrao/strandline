# Figure 2 — study sites along the coast of Chile (six-site benchmark)
# Two north/south panels (Chile is long and narrow) + South America locator.
suppressPackageStartupMessages({
  library(sf); library(rnaturalearth); library(ggplot2)
  library(ggspatial); library(ggrepel); library(patchwork); library(systemfonts)
})

sans_fam <- {
  fams <- unique(system_fonts()$family)
  c("Helvetica","Arial","Nimbus Sans","Liberation Sans","Arimo","DejaVu Sans")[
    c("Helvetica","Arial","Nimbus Sans","Liberation Sans","Arimo","DejaVu Sans") %in% fams][1]
}

sa  <- ne_countries(continent = "South America", scale = "medium", returnclass = "sf")
chl <- ne_countries(country = "Chile", scale = "medium", returnclass = "sf")

sites <- data.frame(
  name = c("Arica","Coquimbo","Viña del Mar","Constitución","Valdivia","Chiloé"),
  lon  = c(-70.31, -71.34, -71.55, -72.41, -73.24, -73.76),
  lat  = c(-18.48, -29.95, -33.02, -35.33, -39.81, -42.48),
  setting = c("arid","semi-arid","central","estuary","estuary","archipelago")
)

acc  <- "#D55E00"   # site marker (Wong vermillion)
land <- "#ECECEC"; landline <- "#9AA0A6"; sea <- "#F7FBFD"

panel <- function(ylim, sdf) {
  ggplot() +
    geom_sf(data = sa, fill = land, color = landline, linewidth = 0.22) +
    geom_point(data = sdf, aes(lon, lat), shape = 21, fill = acc,
               color = "white", stroke = 0.4, size = 2.4) +
    geom_text_repel(data = sdf, aes(lon, lat, label = name),
                    family = sans_fam, size = 2.5, fontface = "bold",
                    color = "#222222", nudge_x = -3.0, hjust = 1,
                    direction = "y", segment.size = 0.3, segment.color = "#7A7A7A",
                    min.segment.length = 0, box.padding = 0.25) +
    annotation_scale(location = "br", width_hint = 0.32, height = unit(1.2, "mm"),
                     text_family = sans_fam, text_cex = 0.55, line_width = 0.4) +
    coord_sf(xlim = c(-78.2, -68.5), ylim = ylim, expand = FALSE) +
    scale_x_continuous(breaks = c(-76, -72)) +
    theme_minimal(base_family = sans_fam) +
    theme(
      panel.background = element_rect(fill = sea, color = NA),
      panel.border = element_rect(color = "grey35", fill = NA, linewidth = 0.5),
      panel.grid = element_line(color = "grey88", linewidth = 0.2),
      axis.title = element_blank(),
      axis.text = element_text(size = 6, color = "grey35"),
      plot.margin = margin(2, 2, 2, 2)
    )
}

p_north <- panel(c(-31, -17), sites[sites$lat >= -31, ])
p_south <- panel(c(-45, -31), sites[sites$lat <  -31, ])

# north arrow only on the north panel (top-right)
p_north <- p_north +
  annotation_north_arrow(location = "tr", height = unit(6, "mm"), width = unit(5, "mm"),
                         style = north_arrow_minimal())

# --- South America locator inset ------------------------------------------
studybox <- st_as_sfc(st_bbox(c(xmin = -78, xmax = -68.5, ymin = -45, ymax = -17),
                              crs = 4326))
locator <- ggplot() +
  geom_sf(data = sa, fill = "#DDE1E4", color = landline, linewidth = 0.15) +
  geom_sf(data = studybox, fill = NA, color = acc, linewidth = 0.6) +
  coord_sf(xlim = c(-82, -34), ylim = c(-56, 13), expand = FALSE) +
  theme_void() +
  theme(panel.background = element_rect(fill = sea, color = "grey50", linewidth = 0.3))

# inset the locator into the north panel's open Pacific (mid-left, clear of
# Arica at the top and Coquimbo at the bottom)
p_north <- p_north +
  inset_element(locator, left = 0.02, bottom = 0.30, right = 0.40, top = 0.70,
                align_to = "panel")

fig <- p_north | p_south

# --- export ---------------------------------------------------------------
W <- 14.0; H <- 11.0
ggsave("figures/sites_map.pdf", fig, width = W, height = H, units = "cm", device = cairo_pdf)
ggsave("figures/sites_map.png", fig, width = W, height = H, units = "cm", dpi = 300, device = ragg::agg_png)
cat(sprintf("wrote figures/sites_map.{pdf,png}  (%.1f x %.1f cm)\n", W, H))
